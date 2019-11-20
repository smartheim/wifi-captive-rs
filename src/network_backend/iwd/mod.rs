//! # The iwd backend
//! See https://git.kernel.org/pub/scm/network/wireless/iwd.git/tree/doc for API documentation.
//!
//! All files in generated/* are auto-generated.
//! Use the `generate.sh` script to update them to newer dbus crate or network dbus API versions.
//!
//! iwd handles connection secrets different than network manager. The network manager API
//! just takes a SSID and a password. iwd requires an "agent" dbus service to be registered.
//! That agent will be asked for credentials for a to-be-established connection.
//!
//! In contrast to NetworkManager you need to assign the hotspot IP to the wifi interface yourself
//! before starting this service. Eg: `ip addr add 192.168.41/24 dev wlan0`
mod generated;

mod access_points_changed;
mod connectivity;
mod credentials_agent;
mod find_wifi_device;

use crate::{dbus_tokio, AccessPointCredentials, ActiveConnection, CaptivePortalError, Connectivity, NetworkManagerState, WifiConnection, SSID, prop_stream, ConnectionState};
pub use access_points_changed::AccessPointsChangedStream;

use dbus::nonblock::SyncConnection;
use dbus::{nonblock, Path};
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use futures_util::StreamExt;
use tokio::stream::StreamExt as TokioStreamExt;
use dbus::arg::RefArg;

pub const NM_BUSNAME: &str = "net.connman.iwd";

#[derive(Clone)]
pub struct NetworkBackend {
    exit_handler: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    pub(crate) conn: Arc<SyncConnection>,
    /// The wifi device. Will always be set, because the service quits if it didn't find a wifi device.
    wifi_device_path: dbus::Path<'static>,
    /// Mac address of the own network interface
    hw: String,
    /// Network interface name
    interface_name: String,
}

impl NetworkBackend {
    /// Create a new connection to the network manager. This will also try to enable networking
    /// and wifi. Returns a network manager instance or an error if no wifi device can be found.
    pub async fn new(
        interface_name: &Option<String>,
    ) -> Result<NetworkBackend, CaptivePortalError> {
        // Prepare an exit handler
        let (exit_handler, exit_receiver) = tokio::sync::oneshot::channel::<()>();

        // Connect to the D-Bus session bus (this is blocking, unfortunately).
        let (resource, conn) = dbus_tokio::new_system_sync()?;

        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        tokio::spawn(async move {
            use futures_util::future::select;
            use futures_util::future::Either;
            use pin_utils::pin_mut;

            pin_mut!(resource);
            pin_mut!(exit_receiver);
            let result = select(resource, exit_receiver).await;
            if let Either::Left((err, _)) = result {
                panic!("Lost connection to D-Bus: {}", err);
            }
        });

        let wifi_device = find_wifi_device::find_wifi_device(conn.clone(), interface_name).await?;
        Ok(NetworkBackend {
            exit_handler: Arc::new(Mutex::new(Some(exit_handler))),
            conn,
            interface_name: wifi_device.interface_name,
            hw: wifi_device.hw,
            wifi_device_path: wifi_device.device_path,
        })
    }

    /// Network might be disabled or "unmanaged". This method tries to enable networking and wifi.
    pub async fn enable_networking_and_wifi(&self) -> Result<(), CaptivePortalError> {
        use generated::device::NetConnmanIwdDevice;
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
        p.set_powered(true).await?;
        Ok(())
    }

    /// Scan for access points if the last scan is older than 10 seconds
    pub async fn scan_networks(&self) -> Result<(), CaptivePortalError> {
        use generated::device::NetConnmanIwdDevice;
        use generated::device::NetConnmanIwdStation;
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
        if p.mode().await? != "station" {
            return Err(CaptivePortalError::NotInStationMode);
        }
        p.scan().await?;
        Ok(())
    }

    /// Terminates this network manager dbus connection
    pub fn quit(self) {
        let mut exit_handler = self
            .exit_handler
            .lock()
            .expect("Lock network manager exit handler mutex");
        if let Some(exit_handler) = exit_handler.take() {
            let _ = exit_handler.send(());
        }
    }

    /// The iwd network state
    pub async fn state(&self) -> Result<NetworkManagerState, CaptivePortalError> {
        use generated::device::NetConnmanIwdDevice;
        use generated::device::NetConnmanIwdStation;
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
        if p.mode().await? != "station" {
            return Err(CaptivePortalError::NotInStationMode);
        }
        let state = NetworkManagerState::from(&p.state().await?[..]);
        Ok(state)
    }

    /// Enables auto connect. This enumerates all known connections and sets auto connect to true.
    pub async fn try_auto_connect(
        &self,
        timeout: std::time::Duration,
    ) -> Result<bool, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, "/", self.conn.clone());
        use generated::iwd::OrgFreedesktopDBusObjectManager;

        // Get all devices (if possible: by interface)
        let objects = p.get_managed_objects().await?;
        for (device_path, entry) in objects {
            if let Some(entry) = entry.get("net.connman.iwd.KnownNetwork") {
                let auto_connect = entry
                    .get("Autoconnect")
                    .ok_or(CaptivePortalError::Generic(
                        "net.connman.iwd.KnownNetwork: Autoconnect expected'",
                    ))?
                    .0
                    .as_any()
                    .downcast_ref::<bool>()
                    .ok_or(CaptivePortalError::Generic(
                        "net.connman.iwd.KnownNetwork/Autoconnect: Expects a bool!",
                    ))?;

                if !auto_connect {
                    use generated::known_network::NetConnmanIwdKnownNetwork;
                    p.set_autoconnect(true);
                }
            }
        }

        let connectivity: Connectivity = self.wait_for_connectivity(false, timeout).await?;
        Ok(connectivity == Connectivity::Full || connectivity == Connectivity::Limited)
    }

    /// Connect to the given SSID with the given credentials.
    /// First tries to find a wifi connection if "hw" is set or "overwrite_same_ssid_connection" is true.
    /// If it finds one, the connection will be altered to use the given credentials and SSID, otherwise a new connection is created.
    ///
    /// If "overwrite_same_ssid_connection" is false and "hw" is not set, but network manager already knows about a connection
    /// to a SSID with the same name, it will add a number suffix to the connection name. If the SSID is
    /// for example "My AP" and there is already a connection with the name "My AP", the newly
    /// created connection will be named "My AP 1".
    ///
    /// # Arguments:
    /// * ssid: The ssid
    /// * credentials: The connection credentials
    /// * hw: The target access point mac address. If this is set, this method will first try to find
    ///   a connection that was connected to that access point in the past and update that connection.
    /// * overwrite_same_ssid_connection: If this is true and a connection can be found that matches the
    ///   given SSID, that connection will be updated.
    pub async fn connect_to(
        &self,
        ssid: SSID,
        credentials: AccessPointCredentials,
        hw: Option<String>,
        overwrite_same_ssid_connection: bool,
    ) -> Result<Option<ActiveConnection>, CaptivePortalError> {
        unimplemented!()
    }

    /// Get access point data for the given access point network manager dbus path.
    pub async fn access_point<'b, P: Into<dbus::Path<'b>>>(
        &self,
        ap_path: P,
    ) -> Result<WifiConnection, CaptivePortalError> {
        let ap_path: Path = ap_path.into();

        unimplemented!()
    }

    /// Return all known access points of the associated wifi device.
    /// The list might not be up to date and can be refreshed with a call to [`scan_networks`].
    ///
    /// ## Arguments
    /// * find_all: Perform a full scan. This may take up to a minute.
    pub async fn list_access_points(
        &self,
        find_all: bool,
    ) -> Result<Vec<WifiConnection>, CaptivePortalError> {
        if find_all {
            self.scan_networks().await?;
        }
        unimplemented!()
    }

    /// iwd does not store hotspot/APs as "known network"s, so there is nothing to deactivate.
    /// This method will however change from hotspot/AP mode into station mode if necessary.
    pub async fn deactivate_hotspots(&self) -> Result<(), CaptivePortalError> {
        use generated::device::NetConnmanIwdDevice;
        use generated::device::NetConnmanIwdStation;
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
        if p.mode().await? != "station" {
            p.set_mode("station".into()).await?;
        }
        Ok(())
    }

    /// Starts a hotspot
    pub async fn hotspot_start(
        &self,
        ssid: SSID,
        password: Option<String>,
        address: Option<Ipv4Addr>,
    ) -> Result<ActiveConnection, CaptivePortalError> {
        use generated::device::NetConnmanIwdDevice;
        use generated::device::NetConnmanIwdAccessPoint;
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
        if p.mode().await? != "ap" {
            p.set_mode("ap".into()).await?;
        }

        info!("Configuring hotspot ...");
        p.start(&ssid, &password.unwrap_or_default()).await?;

        // Wait for connected state
        let stream = prop_stream(&self.wifi_device_path, self.conn.clone()).await?;

        let mut stream = stream.timeout(Duration::from_secs(1));
        loop {
            let v = stream.next().await;
            match v {
                Some(Ok((value, _path))) => {
                    if &value.interface_name[..] == "net.connman.iwd.AccessPoint" {
                        if value.changed_properties.contains_key("Started") {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }

        let state = p.started().await?;

        Ok(ActiveConnection {
            connection_path: self.wifi_device_path.clone(),
            active_connection_path: self.wifi_device_path.clone(),
            state: match state { true => ConnectionState::Activated, false => ConnectionState::Deactivated },
        })
    }

    pub async fn on_hotspot_stopped(&self, path: dbus::Path<'_>) -> Result<(), CaptivePortalError> {
        unimplemented!()
    }
}
