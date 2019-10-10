//! Network manager interface via dbus
//!
//! All files except the mod.rs file are auto-generated.
//! Use the `generate.sh` script to update them to newer dbus crate or network dbus API versions.

mod connectivity;
mod dbus_types;
mod generated;
mod security;
mod utils;
mod wifi_settings;
mod dbus_tokio;

pub use dbus_types::*;
pub use generated::*;
pub use security::{AccessPointCredentials, credentials_from_data};
pub use connectivity::{NetworkManagerState, ConnectionState};

//use dbus::channel::MatchingReceiver;
//use dbus::message::MatchRule;

use dbus::nonblock;

use super::CaptivePortalError;
use dbus::nonblock::SyncConnection;
use serde::Serialize;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use crate::nm::wifi_settings::{get_connection_settings, WifiConnectionMode};
use core::fmt;
use chrono::{Utc, TimeZone};
use dbus::message::SignalArgs;
use futures_util::StreamExt;
use bitflags::_core::time::Duration;

pub const NM_INTERFACE: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";

pub const NM_SETTINGS_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings";
pub const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager";

pub const NM_CONNECTION_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings.\
                                           Connection";
pub const NM_ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const NM_DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";
pub const NM_WIRELESS_INTERFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub const NM_ACCESS_POINT_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";

pub const UNKNOWN_CONNECTION: &str = "org.freedesktop.NetworkManager.UnknownConnection";
pub const METHOD_RETRY_ERROR_NAMES: &[&str; 1] = &[UNKNOWN_CONNECTION];

/// A wifi SSID
/// According to last standard 802.11-2012 (Section 6.3.11.2.2),
/// a SSID  can be 0-32 octets with an unspecified or UTF8 encoding.
pub type SSID = String;

#[derive(Serialize, Clone, Debug)]
pub struct WifiConnection {
    pub ssid: SSID,
    /// The unique hw address of the access point
    pub hw: String,
    // The wifi mode
    pub security: &'static str,
    // The signal strength
    pub strength: u8,
    // The frequency
    pub frequency: u32,
}

#[derive(Serialize, Debug)]
pub enum NetworkManagerEvent {
    Added,
    Removed,
}


impl fmt::Display for NetworkManagerEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}


#[derive(Serialize)]
pub struct WifiConnectionEvent {
    pub connection: WifiConnection,
    pub event: NetworkManagerEvent,
}

#[derive(Serialize)]
pub struct WifiConnections(pub Vec<WifiConnection>);

#[derive(Clone)]
pub struct NetworkManager {
    exit_handler: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    conn: Arc<SyncConnection>,
    /// The wifi device. Will always be set, because the service quits if it didn't find a wifi device.
    wifi_device_path: String,
    interface_name: String,
}

pub struct ActiveConnection {
    pub connection_path: String,
    pub active_connection_path: String,
    pub state: ConnectionState,
}


pub struct AccessPointChanged {
    pub path: String,
    pub event: NetworkManagerEvent,
}

type APAddedType = dbus_tokio::SignalStream<device::OrgFreedesktopNetworkManagerDeviceWirelessAccessPointAdded, AccessPointChanged>;
type APRemovedType = dbus_tokio::SignalStream<device::OrgFreedesktopNetworkManagerDeviceWirelessAccessPointRemoved, AccessPointChanged>;
pub type AccessPointChangeReturnType = futures_util::stream::Select<APAddedType, APRemovedType>;

impl NetworkManager {
    /// Create a new connection to the network manager. This will also try to enable networking
    /// and wifi.
    pub async fn new(config: &super::config::Config) -> Result<NetworkManager, CaptivePortalError> {
        // Prepare an exit handler
        let (exit_handler, exit_receiver) = tokio::sync::oneshot::channel::<()>();

        // Connect to the D-Bus session bus (this is blocking, unfortunately).
        let (resource, conn) = dbus_tokio::new_system_sync()?;

        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        tokio::spawn(async move {
            use futures_util::future::Either;
            use futures_util::future::select;
            use pin_utils::pin_mut;

            pin_mut!(resource);
            pin_mut!(exit_receiver);
            let result = select(resource, exit_receiver).await;
            if let Either::Left((err, _)) = result {
                panic!("Lost connection to D-Bus: {}", err);
            }
        });

        {
            let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, conn.clone());
            use networkmanager::NetworkManager;
            if !p.networking_enabled().await? {
                p.enable(true).await?;
            }
            if !p.wireless_enabled().await? {
                p.set_wireless_enabled(true).await?;
            }
        }

        let (wifi_device_path, interface_name) =
            utils::find_wifi_device(conn.clone(), &config.interface).await?;
        Ok(NetworkManager {
            exit_handler: Arc::new(Mutex::new(Some(exit_handler))),
            conn,
            interface_name,
            wifi_device_path,
        })
    }

    /// The active wifi connection path, if any.
    pub async fn wifi_active_connection(&self) -> Option<dbus::Path<'_>> {
        let p = nonblock::Proxy::new(NM_INTERFACE, &self.wifi_device_path, self.conn.clone());
        use device::OrgFreedesktopNetworkManagerDevice;
        if let Ok(path) = p.active_connection().await {
            Some(path)
        } else {
            None
        }
    }

    /// Scan for access points if the last scan is older than 10 seconds
    pub async fn scan_networks(&self) -> Result<(), CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_INTERFACE, &self.wifi_device_path, self.conn.clone());
        use device::OrgFreedesktopNetworkManagerDeviceWireless;
        use chrono::Duration;
        let last_scan = chrono::Utc::now() - Utc.timestamp(p.last_scan().await?, 0);
        if last_scan > Duration::seconds(10) {
            p.request_scan().await?;
        }
        Ok(())
    }

    /// Terminates this network manager dbus connection
    pub fn quit(self) {
        let mut eh = self.exit_handler.lock().unwrap();
        if let Some(eh) = eh.take() {
            let _ = eh.send(());
        }
    }

    pub async fn create_start_hotspot(
        &self,
        ssid: SSID,
        password: Option<String>,
        address: Option<Ipv4Addr>,
    ) -> Result<dbus::Path<'_>, CaptivePortalError> {
        // Remove existing connection
        {
            let p = nonblock::Proxy::new(NM_SETTINGS_INTERFACE, NM_SETTINGS_PATH, self.conn.clone());
            let connection_paths = {
                use connections::OrgFreedesktopNetworkManagerSettings;
                p.connections().await?
            };
            use connection_nm::OrgFreedesktopNetworkManagerSettingsConnection;
            for conn_path in connection_paths {
                if let Some(settings) = get_connection_settings(self.conn.clone(), conn_path).await? {
                    if settings.mode == WifiConnectionMode::AP && settings.ssid == ssid {
                        info!("Delete existing network manager hotspot configuration");
                        p.delete().await?;
                    }
                }
            }
        }

        let settings =
            wifi_settings::make_arguments_for_sta(ssid, password, address, &self.interface_name)?;

        let p = nonblock::Proxy::new(NM_INTERFACE, &self.wifi_device_path, self.conn.clone());

        use networkmanager::NetworkManager;
        let (conn_path, active_connection) = p
            .add_and_activate_connection(
                settings,
                dbus::Path::new(self.wifi_device_path.as_bytes())?,
                dbus::Path::new("/")?,
            )
            .await?;

        let state_after_wait = self
            .wait_for_active_connection_state(
                connectivity::ConnectionState::Activated,
                active_connection,
                std::time::Duration::from_millis(5000),
            )
            .await?;

        if state_after_wait != connectivity::ConnectionState::Activated {
            return Err(CaptivePortalError::hotspot_failed());
        }

        Ok(conn_path)
    }

//    pub fn on_access_point_changes(&self) -> Result<dbus_tokio::SignalStream<access_point::OrgFreedesktopNetworkManagerAccessPointPropertiesChanged>, CaptivePortalError> {
//        use dbus_tokio::SignalStream;
//        use access_point::OrgFreedesktopNetworkManagerAccessPointPropertiesChanged as APChanged;
//        let rule = APChanged::match_rule(Some(&NM_INTERFACE.to_owned().into()), None).static_clone();
//        SignalStream::new(self.conn.clone(), rule)
//    }

    /// Subscribe to access point added and removed bus signals.
    pub async fn on_access_point_list_changes(&self) -> Result<AccessPointChangeReturnType, CaptivePortalError> {
        /// This is implemented via stream merging, because each subscription is encapsulated in its own stream.
        use dbus_tokio::SignalStream;
        use device::OrgFreedesktopNetworkManagerDeviceWirelessAccessPointAdded as APAdded;
        use device::OrgFreedesktopNetworkManagerDeviceWirelessAccessPointRemoved as APRemoved;
        let rule_added = APAdded::match_rule(Some(&NM_INTERFACE.to_owned().into()), Some(&self.wifi_device_path.clone().into())).static_clone();
        let stream_added: SignalStream<APAdded, AccessPointChanged> =
            SignalStream::new(self.conn.clone(), rule_added,
                              Box::new(|v: APAdded| {
                                  AccessPointChanged {
                                      event: NetworkManagerEvent::Added,
                                      path: v.access_point.to_string(),
                                  }
                              })).await?;

        let rule_removed = APRemoved::match_rule(Some(&NM_INTERFACE.to_owned().into()), Some(&self.wifi_device_path.clone().into())).static_clone();
        let stream_removed: SignalStream<APRemoved, AccessPointChanged> =
            SignalStream::new(self.conn.clone(), rule_removed,
                              Box::new(|v: APRemoved| {
                                  AccessPointChanged {
                                      event: NetworkManagerEvent::Added,
                                      path: v.access_point.to_string(),
                                  }
                              })).await?;
        let r = futures_util::stream::select(stream_added, stream_removed);
        Ok(r)
    }

    pub async fn print_connection_changes(&self) -> Result<(), CaptivePortalError> {
        use dbus_tokio::SignalStream;
        use connection_active::OrgFreedesktopNetworkManagerConnectionActiveStateChanged as ConnectionActiveChanged;

        let rule = ConnectionActiveChanged::match_rule(None, None).static_clone();
        let stream: SignalStream<ConnectionActiveChanged, ConnectionActiveChanged> = SignalStream::new(self.conn.clone(), rule,
                                                                                                           Box::new(|v| { v })).await?;
        pin_utils::pin_mut!(stream);

        while let Some((value, path)) = stream.next().await {
            println!("Connection state changed: {:?} {} on {}", ConnectionState::from(value.state), value.reason, path);
        }

        Ok(())
    }

    pub async fn wait_until_connection_lost(
        &self,
    ) -> Result<connectivity::NetworkManagerState, CaptivePortalError> {
        use dbus_tokio::SignalStream;
        use networkmanager::NetworkManagerStateChanged as StateChanged;

        // Not connected in the first place
        let state = self.state().await?;
        if state != NetworkManagerState::ConnectedGlobal && state != NetworkManagerState::ConnectedLocal && state != NetworkManagerState::ConnectedSite {
            info!("Not connected right now: {:?}", state);
            return Ok(state);
        }

        let rule = StateChanged::match_rule(None, None).static_clone();
        let stream: SignalStream<StateChanged, StateChanged> = SignalStream::new(self.conn.clone(), rule, Box::new(|v| { v })).await?;
        pin_utils::pin_mut!(stream);

        while let Some((value, path)) = stream.next().await {
            let state = NetworkManagerState::from(value.state);
            if state != NetworkManagerState::ConnectedGlobal && state != NetworkManagerState::ConnectedLocal && state != NetworkManagerState::ConnectedSite {
                info!("Connection state changed: {:?}", state);
                return Ok(state);
            }
        }

        Ok(NetworkManagerState::Unknown)
    }

    /// The returned future resolves when either the timeout expired or state of the
    /// **active** connection (eg /org/freedesktop/NetworkManager/ActiveConnection/12) is the or changes into
    /// the expected state.
    pub async fn wait_for_active_connection_state(
        &self,
        expected_state: connectivity::ConnectionState,
        path: dbus::Path<'_>,
        timeout: std::time::Duration,
    ) -> Result<connectivity::ConnectionState, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_ACTIVE_INTERFACE, path, self.conn.clone());

        use connection_active::OrgFreedesktopNetworkManagerConnectionActive;
        let state: connectivity::ConnectionState = p.state().await?.into();
        if state == expected_state {
            return Ok(state);
        }

        use dbus_tokio::SignalStream;
        use connection_active::OrgFreedesktopNetworkManagerConnectionActiveStateChanged as StateChanged;

        let rule = StateChanged::match_rule(None, None).static_clone();
        let stream: SignalStream<StateChanged, u32> = SignalStream::new(self.conn.clone(), rule, Box::new(|v: StateChanged| { v.state })).await?;
        pin_utils::pin_mut!(stream);

        while let Some(state_change) = crate::utils::timed_future(stream.next(), timeout).await {
            if let Some((state, path)) = state_change {
                let state = connectivity::ConnectionState::from(state);
                if state == expected_state {
                    return Ok(state);
                }
            }
        }

        let state: connectivity::ConnectionState = p.state().await?.into();
        Ok(state)
    }

    /// The current connectity status
    pub async fn connectivity(&self) -> Result<connectivity::Connectivity, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, self.conn.clone());
        use networkmanager::NetworkManager;
        Ok(p.connectivity().await?.into())
    }

    /// The network manager state
    pub async fn state(&self) -> Result<connectivity::NetworkManagerState, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, self.conn.clone());
        use networkmanager::NetworkManager;
        Ok(p.state().await?.into())
    }

    /// Disables all known wifi connections, set auto-connect to true and enable them again
    /// and let network manager try to auto-connect.
    pub async fn try_auto_connect(&self, timeout: std::time::Duration) -> Result<Option<ActiveConnection>, CaptivePortalError> {
        let connections = {
            use connections::OrgFreedesktopNetworkManagerSettings;
            let p = nonblock::Proxy::new(NM_INTERFACE, NM_SETTINGS_PATH, self.conn.clone());
            p.list_connections().await?
        };
        for connection_path in connections {
            let settings = wifi_settings::get_connection_settings(self.conn.clone(), connection_path.clone()).await?;
            if let Some(settings) = settings {
                let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, self.conn.clone());
                use networkmanager::NetworkManager;
                p.activate_connection(connection_path, (&self.wifi_device_path).into(), "".into());
            }
        }

        Ok(None)
    }

    /// Returns a tuple with network manager dbus paths on success: (connection, active_connection)
    pub async fn update_connection(&self,
                                   ssid: SSID,
                                   hw: String,
                                   credentials: security::AccessPointCredentials, ) -> Result<Option<(dbus::Path<'_>, dbus::Path<'_>)>, CaptivePortalError> {
        let connections = {
            use connections::OrgFreedesktopNetworkManagerSettings;
            let p = nonblock::Proxy::new(NM_INTERFACE, NM_SETTINGS_PATH, self.conn.clone());
            p.list_connections().await?
        };
        for connection_path in connections {
            let settings = wifi_settings::get_connection_settings(self.conn.clone(), connection_path).await?;
            if let Some(settings) = settings {
                // A matching connection could be found. Replace the settings with new ones and store to disk
                if settings.seen_bssids.contains(&hw) {
                    use connection_nm::OrgFreedesktopNetworkManagerSettingsConnection;
                    let p = nonblock::Proxy::new(NM_INTERFACE, connection_path.clone(), self.conn.clone());
                    const SAVE_TO_DISK_FLAG: u32 = 0x01;
                    let settings = wifi_settings::make_arguments_for_ap::<&'static str>(ssid, credentials)?;
                    p.update2(settings, SAVE_TO_DISK_FLAG, VariantMap::new()).await?;
                    // Activate connection
                    let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, self.conn.clone());
                    use networkmanager::NetworkManager;
                    let active_path = p.activate_connection(connection_path.clone(), self.wifi_device_path.into(), "".into()).await?;
                    return Ok(Some((connection_path, active_path)));
                }
            }
        }
        Ok(None)
    }

    /// Connect to the given SSID with the given credentials.
    /// First tries to find a wifi connection that was connected to an access point with the given "hw" (mac address).
    /// If it finds one, the connection will be altered to use the given credentials and SSID, otherwise a new connection is created.
    pub async fn connect_to(
        &self,
        ssid: SSID,
        hw: Option<String>,
        credentials: security::AccessPointCredentials,
    ) -> Result<Option<ActiveConnection>, CaptivePortalError> {
        // try to find connection, update it, activate it and return the connection path
        let active_connection = if let Some(hw) = hw {
            self.update_connection(ssid, hw, credentials).await?
        } else {
            None
        };

        // If not found: Create and activate a new connection
        let (connection_path, active_connection) = if active_connection.is_none() {
            let settings = wifi_settings::make_arguments_for_ap(ssid, credentials)?;
            let options = wifi_settings::make_options_for_ap();

            // Create connection
            use networkmanager::NetworkManager;
            let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, self.conn.clone());
            let (conn_path, active_connection, _) = p.add_and_activate_connection2(settings, self.wifi_device_path.into(), "".into(), options).await?;
            (conn_path, active_connection)
        } else {
            active_connection.unwrap()
        };

        // Wait until connected
        let state = self.wait_for_active_connection_state(
            connectivity::ConnectionState::Activated,
            active_connection,
            Duration::from_secs(7),
        ).await?;

        // Remove connection if not successful
        if state == connectivity::ConnectionState::Activated {
            use connection_nm::OrgFreedesktopNetworkManagerSettingsConnection;
            let p = nonblock::Proxy::new(NM_INTERFACE, connection_path, self.conn.clone());
            //  flags: optional flags argument.
            // Currently supported flags are: "0x1" (to-disk), "0x2" (in-memory), "0x4" (in-memory-detached),
            // "0x8" (in-memory-only), "0x10" (volatile), "0x20" (block-autoconnect), "0x40" (no-reapply).
            const SAVE_TO_DISK_FLAG: u32 = 0x01;
            // Settings: Provide an empty array, to use the current settings.
            p.update2(VariantMapNested::new(), SAVE_TO_DISK_FLAG, VariantMap::new()).await?;
            return Ok(Some(ActiveConnection {
                connection_path: connection_path.to_string(),
                active_connection_path: active_connection.to_string(),
                state,
            }));
        } else {
            use connection_nm::OrgFreedesktopNetworkManagerSettingsConnection;
            let p = nonblock::Proxy::new(NM_INTERFACE, connection_path, self.conn.clone());
            p.delete().await?;
            return Ok(None);
        }
    }

    pub async fn access_point<'a, P: Into<dbus::Path<'a>>>(&self, ap_path: P) -> Result<WifiConnection, CaptivePortalError> {
        let ap_path = ap_path.into();
        let security = security::get_access_point_security(self.conn.clone(), &ap_path)
            .await?
            .as_str();
        let access_point_data = nonblock::Proxy::new(NM_INTERFACE, ap_path, self.conn.clone());
        use access_point::OrgFreedesktopNetworkManagerAccessPoint;
        let hw = access_point_data.hw_address().await?;
        let ssid = String::from_utf8(access_point_data.ssid().await?)?;

        let wifi_connection = WifiConnection {
            ssid,
            hw,
            security,
            strength: access_point_data.strength().await?,
            frequency: access_point_data.frequency().await?,
        };
        info!("ap {:?}", &wifi_connection);
        Ok(wifi_connection)
    }

    /// Return all known access points of the associated wifi device.
    /// The list might not be up to date and can be refreshed with a call to [`scan_networks`].
    pub async fn list_access_points(&self) -> Result<Vec<WifiConnection>, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_INTERFACE, &self.wifi_device_path, self.conn.clone());

        let access_point_paths = {
            use device::OrgFreedesktopNetworkManagerDeviceWireless;
            p.get_all_access_points().await?
        };

        let mut connections = Vec::with_capacity(access_point_paths.len());
        for ap_path in access_point_paths {
            connections.push(self.access_point(ap_path).await?);
        }

        Ok(connections)
    }
}
