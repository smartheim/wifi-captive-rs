//! Network manager interface via dbus
//!
//! All files in generated/* are auto-generated.
//! Use the `generate.sh` script to update them to newer dbus crate or network dbus API versions.

mod access_points_changed;
mod connectivity;
mod device_state_type;
mod find_connection;
mod find_wifi_device;
mod generated;
mod hotspot;
mod security;
mod wifi_settings;

use dbus::{nonblock, nonblock::SyncConnection};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::delay_for;

// Re-export for easier use in sub-modules
use crate::dbus_tokio;
use crate::network_interface::{
    AccessPointCredentials, ActiveConnection, ConnectionState, NetworkManagerState, WifiConnection, SSID,
};
use crate::CaptivePortalError;
use generated::*;
use wifi_settings::{VariantMap, VariantMapNested};

// Public API: AccessPointsChangedStream
pub use access_points_changed::{ap_changed_stream, AccessPointChanged};

pub const NM_BUSNAME: &str = "org.freedesktop.NetworkManager";
pub(crate) const NM_PATH: &str = "/org/freedesktop/NetworkManager";
pub(crate) const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
pub(crate) const HOTSPOT_UUID: &str = "2b0d0f1d-b79d-43af-bde1-71744625642e";

// Connection flags: optional flags argument.
// Currently supported flags are: "0x1" (to-disk), "0x2" (in-memory), "0x4" (in-memory-detached),
// "0x8" (in-memory-only), "0x10" (volatile), "0x20" (block-autoconnect), "0x40" (no-reapply).
pub const SAVE_TO_DISK_FLAG: u32 = 0x01;
pub const VOLATILE_FLAG: u32 = 0x8 | 0x10;
pub const IN_MEMORY_ONLY: u32 = 0x8 | 0x20;

#[derive(Clone)]
pub struct NetworkBackend {
    exit_handler: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    pub(crate) conn: Arc<SyncConnection>,
    /// The wifi device. Will always be set, because the service quits if it didn't find a wifi device.
    pub(crate) wifi_device_path: dbus::Path<'static>,
    /// Mac address of the own network interface
    hw: String,
    /// Network interface name
    interface_name: String,
}

impl NetworkBackend {
    /// Create a new connection to the network manager. This will also try to enable networking
    /// and wifi. Returns a network manager instance or an error if no wifi device can be found.
    pub async fn new(interface_name: &Option<String>) -> Result<NetworkBackend, CaptivePortalError> {
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
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
        use networkmanager::NetworkManager;
        if !p.networking_enabled().await? {
            p.enable(true).await?;
        }
        if !p.wireless_enabled().await? {
            p.set_wireless_enabled(true).await?;
        }
        if p.connectivity_check_available().await? {
            p.set_connectivity_check_enabled(true).await?;
        }
        Ok(())
    }

    /// Scan for access points if the last scan is older than 10 seconds
    pub async fn scan_networks(&self) -> Result<(), CaptivePortalError> {
        use generated::device::DeviceWireless;
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());

        // request_scan requires a hashmap of dbus::arg::RefArg parameters as argument.
        // Those are not thread safe, eg implement Send, so cannot be wrapped as intermediate state in the
        // async state machine. A function scope helps out here.
        fn scan_networks(p: dbus::nonblock::Proxy<Arc<SyncConnection>>) -> dbus::nonblock::MethodReply<()> {
            p.request_scan(HashMap::new())
        }

        // There is one error that we can expect by calling this method:
        // org.freedesktop.NetworkManager.Device.NotAllowed - Scanning not allowed while already scanning
        if let Err(e) = scan_networks(p).await {
            if let Some(name) = e.name() {
                // All good
                if name == "org.freedesktop.NetworkManager.Device.NotAllowed" {
                    return Ok(());
                }
            }
            return Err(e.into());
        }

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

    /// The network manager state
    pub async fn state(&self) -> Result<NetworkManagerState, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
        use networkmanager::NetworkManager;
        Ok(NetworkManagerState::from(p.state().await?))
    }

    /// Let network manager try to auto-connect.
    pub async fn try_auto_connect(&self, timeout: std::time::Duration) -> Result<bool, CaptivePortalError> {
        self.enable_auto_connect().await;

        use connections::Settings;
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_SETTINGS_PATH, self.conn.clone());

        debug!(
            "Trying to connect to one of {} known connections ...",
            p.connections().await?.len()
        );

        match self.wait_for_connectivity(false, timeout).await {
            Ok(state) => Ok(state == NetworkManagerState::Connected || state == NetworkManagerState::ConnectedLimited),
            Err(CaptivePortalError::NotRequiredConnectivity(_)) => Ok(false),
            Err(e) => Err(e),
        }
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
        // try to find connection, update it, activate it and return the connection path
        let active_connection = if let Some(hw) = hw {
            if let Some((connection_path, old_connection)) = self.find_connection_by_mac(&hw).await? {
                Some(
                    self.update_connection(connection_path, &ssid, old_connection, credentials.clone())
                        .await?,
                )
            } else {
                None
            }
        } else if overwrite_same_ssid_connection {
            if let Some((connection_path, old_connection)) = self.find_connection_by_ssid(&ssid).await? {
                Some(
                    self.update_connection(connection_path, &ssid, old_connection, credentials.clone())
                        .await?,
                )
            } else {
                None
            }
        } else {
            None
        };

        // If not found: Create and activate a new connection
        let (connection_path, active_connection) = if let Some(active_connection) = active_connection {
            active_connection
        } else {
            let settings = wifi_settings::make_arguments_for_ap(&ssid, credentials, None)?;
            let options = wifi_settings::make_options_for_ap();

            // Create connection
            use networkmanager::NetworkManager;
            let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
            let (conn_path, active_connection, _) = p
                .add_and_activate_connection2(settings, self.wifi_device_path.clone(), "/".into(), options)
                .await?;
            (conn_path, active_connection)
        };

        // Wait up to 5 seconds while in Deactivated
        let state = self
            .wait_for_active_connection_state(
                ConnectionState::Deactivated,
                active_connection.clone(),
                Duration::from_secs(10),
                true,
            )
            .await?;
        // Not successful
        if state == ConnectionState::Deactivated {
            use connection_nm::Connection;
            let p = nonblock::Proxy::new(NM_BUSNAME, connection_path, self.conn.clone());
            p.delete().await?;
            return Ok(None);
        }

        // Wait up to 30 seconds while in Activating
        let state = self
            .wait_for_active_connection_state(
                ConnectionState::Activated,
                active_connection.clone(),
                Duration::from_secs(30),
                false,
            )
            .await?;

        // Remove connection if not successful. Store it permanently if successful
        if state == ConnectionState::Activated {
            use connection_nm::Connection;
            let p = nonblock::Proxy::new(NM_BUSNAME, connection_path.clone(), self.conn.clone());

            // Settings: Provide an empty array, to use the current settings.
            p.update2(VariantMapNested::new(), SAVE_TO_DISK_FLAG, VariantMap::new())
                .await?;
            return Ok(Some(ActiveConnection {
                connection_path: connection_path.into_static(),
                active_connection_path: active_connection.into_static(),
                state,
            }));
        } else {
            use connection_nm::Connection;
            let p = nonblock::Proxy::new(NM_BUSNAME, connection_path, self.conn.clone());
            p.delete().await?;
            return Ok(None);
        }
    }

    /// Get access point data for the given access point network manager dbus path.
    pub async fn access_point<'b, P: Into<dbus::Path<'b>>>(
        &self,
        ap_path: P,
    ) -> Result<WifiConnection, CaptivePortalError> {
        let ap_path = ap_path.into();
        let security = security::get_access_point_security(self.conn.clone(), &ap_path)
            .await?
            .as_str();
        let access_point_data = nonblock::Proxy::new(NM_BUSNAME, ap_path, self.conn.clone());
        use access_point::AccessPoint;
        let hw = access_point_data.hw_address().await?;
        let ssid = String::from_utf8(access_point_data.ssid().await?)?;

        let wifi_connection = WifiConnection {
            is_own: hw == self.hw,
            ssid,
            hw,
            security,
            strength: access_point_data.strength().await?,
            frequency: access_point_data.frequency().await?,
        };
        if !wifi_connection.is_own {
            info!("Found AP {:?}", &wifi_connection.ssid);
        }
        Ok(wifi_connection)
    }

    /// Return all known access points of the associated wifi device.
    /// The list might not be up to date and can be refreshed with a call to [`scan_networks`].
    ///
    /// ## Arguments
    /// * timeout: If timeout is != 0, performs a full scan. Waits up to timeout for at least one result.
    pub async fn list_access_points(
        &self,
        mut timeout: std::time::Duration,
    ) -> Result<Vec<WifiConnection>, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());

        let connections = {
            use device::DeviceWireless;
            if timeout.as_secs() > 0 {
                self.scan_networks().await?;
            }
            let interval = Duration::from_millis(500);
            loop {
                // Get access point list
                let stream = futures_util::stream::iter(p.get_access_points().await?.into_iter());
                let access_point_paths: Vec<WifiConnection> = stream
                    .filter_map(|ap_path| {
                        async {
                            self.access_point(ap_path).await.ok().and_then(|ap| match ap.is_own {
                                true => None,
                                false => Some(ap),
                            })
                        }
                    })
                    .collect()
                    .await;
                if access_point_paths.len() > 0 {
                    break access_point_paths;
                }
                if timeout.as_millis() <= 0 {
                    break access_point_paths;
                }
                delay_for(interval).await;
                timeout -= interval;
            }
        };

        Ok(connections)
    }
}
