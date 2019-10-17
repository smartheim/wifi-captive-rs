//! Network manager interface via dbus
//!
//! All files except the mod.rs file are auto-generated.
//! Use the `generate.sh` script to update them to newer dbus crate or network dbus API versions.

mod connectivity;
mod dbus_tokio;
mod find_wifi_device;
mod generated;
mod security;
mod wifi_settings;
mod hotspot;

use super::CaptivePortalError;
pub use connectivity::{
    ConnectionState, Connectivity, NetworkManagerState, NETWORK_MANAGER_STATE_CONNECTED,
    NETWORK_MANAGER_STATE_NOT_CONNECTED, NETWORK_MANAGER_STATE_TEMP,
    wait_for_active_connection_state, wait_until_state, on_active_connection_state_change,
    wait_for_connectivity, print_connection_changes,
};
pub use generated::*;
pub use security::{credentials_from_data, AccessPointCredentials};
use wifi_settings::{VariantMap, VariantMapNested, WifiConnectionMode};

use dbus::{message::SignalArgs, nonblock, nonblock::SyncConnection};

use bitflags::_core::time::Duration;
use chrono::{TimeZone, Utc};
use core::fmt;
use serde::Serialize;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use crate::nm::wifi_settings::WiFiConnectionSettings;

pub const NM_BUSNAME: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
pub const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const HOTSPOT_UUID: &str = "2b0d0f1d-b79d-43af-bde1-71744625642e";

// Connection flags: optional flags argument.
// Currently supported flags are: "0x1" (to-disk), "0x2" (in-memory), "0x4" (in-memory-detached),
// "0x8" (in-memory-only), "0x10" (volatile), "0x20" (block-autoconnect), "0x40" (no-reapply).
const SAVE_TO_DISK_FLAG: u32 = 0x01;
const VOLATILE_FLAG: u32 = 0x8 | 0x10;
const IN_MEMORY_ONLY: u32 = 0x8 | 0x20;

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
    wifi_device_path: dbus::Path<'static>,
    /// Mac address of the own network interface
    hw: String,
    /// Network interface name
    interface_name: String,
}

pub struct ActiveConnection {
    pub connection_path: dbus::Path<'static>,
    pub active_connection_path: dbus::Path<'static>,
    pub state: ConnectionState,
}

pub struct AccessPointChanged {
    pub path: String,
    pub event: NetworkManagerEvent,
}

type APAddedType =
dbus_tokio::SignalStream<device::DeviceWirelessAccessPointAdded, AccessPointChanged>;
type APRemovedType =
dbus_tokio::SignalStream<device::DeviceWirelessAccessPointRemoved, AccessPointChanged>;
pub type AccessPointChangeReturnType = futures_util::stream::Select<APAddedType, APRemovedType>;

impl NetworkManager {
    /// Create a new connection to the network manager. This will also try to enable networking
    /// and wifi. Returns a network manager instance or an error if no wifi device can be found.
    pub async fn new(
        interface_name: &Option<String>,
    ) -> Result<NetworkManager, CaptivePortalError> {
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
        Ok(NetworkManager {
            exit_handler: Arc::new(Mutex::new(Some(exit_handler))),
            conn,
            interface_name: wifi_device.interface_name,
            hw: wifi_device.hw,
            wifi_device_path: wifi_device.device_path,
        })
    }

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
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
        use chrono::Duration;
        use device::DeviceWireless;
        let last_scan = chrono::Utc::now() - Utc.timestamp(p.last_scan().await?, 0);
        if last_scan > Duration::seconds(10) {
            // request_scan requires a hashmap of dbus::arg::RefArg parameters as argument.
            // Those are not thread safe, eg implement Send, so cannot be wrapped as intermediate state in the
            // async state machine. A function scope helps out here.
            fn scan_networks(
                p: dbus::nonblock::Proxy<Arc<SyncConnection>>,
            ) -> dbus::nonblock::MethodReply<()> {
                p.request_scan(HashMap::new())
            }
            scan_networks(p).await?;
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

    /// Subscribe to access point added and removed bus signals.
    pub async fn on_access_point_list_changes(
        &self,
    ) -> Result<AccessPointChangeReturnType, CaptivePortalError> {
        /// This is implemented via stream merging, because each subscription is encapsulated in its own stream.
        use dbus_tokio::SignalStream;
        use device::DeviceWirelessAccessPointAdded as APAdded;
        use device::DeviceWirelessAccessPointRemoved as APRemoved;
        let rule_added = APAdded::match_rule(
            Some(&NM_BUSNAME.to_owned().into()),
            Some(&self.wifi_device_path.clone().into()),
        )
            .static_clone();
        let stream_added: SignalStream<APAdded, AccessPointChanged> = SignalStream::new(
            self.conn.clone(),
            rule_added,
            Box::new(|v: APAdded| AccessPointChanged {
                event: NetworkManagerEvent::Added,
                path: v.access_point.to_string(),
            }),
        )
            .await?;

        let rule_removed = APRemoved::match_rule(
            Some(&NM_BUSNAME.to_owned().into()),
            Some(&self.wifi_device_path.clone().into()),
        )
            .static_clone();
        let stream_removed: SignalStream<APRemoved, AccessPointChanged> = SignalStream::new(
            self.conn.clone(),
            rule_removed,
            Box::new(|v: APRemoved| AccessPointChanged {
                event: NetworkManagerEvent::Removed,
                path: v.access_point.to_string(),
            }),
        )
            .await?;
        let r = futures_util::stream::select(stream_added, stream_removed);
        Ok(r)
    }

    /// The current connectity status
    //    pub async fn connectivity(&self) -> Result<connectivity::Connectivity, CaptivePortalError> {
    //        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
    //        use networkmanager::NetworkManager;
    //        Ok(p.connectivity().await?.into())
    //    }

    /// The network manager state
    pub async fn state(&self) -> Result<connectivity::NetworkManagerState, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
        use networkmanager::NetworkManager;
        Ok(p.state().await?.into())
    }

    /// Let network manager try to auto-connect.
    pub async fn try_auto_connect(
        &self,
        timeout: std::time::Duration,
    ) -> Result<bool, CaptivePortalError> {
        self.enable_auto_connect().await;

        use connections::Settings;
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_SETTINGS_PATH, self.conn.clone());

        info!("Trying to connect to one of {} known connections ...", p.connections().await?.len());

        let state = wait_until_state(self, *NETWORK_MANAGER_STATE_CONNECTED, Some(timeout), false)
            .await?;

        Ok(NETWORK_MANAGER_STATE_CONNECTED.contains(state))
    }

    /// Returns the dbus network manager api connection path and old connection settings as tuple.
    pub async fn find_connection_by_mac(&self, hw: &String) -> Result<Option<(dbus::Path<'_>,WiFiConnectionSettings)>, CaptivePortalError> {
        let connections = {
            use connections::Settings;
            let p = nonblock::Proxy::new(NM_BUSNAME, NM_SETTINGS_PATH, self.conn.clone());
            p.connections().await?
        };
        for connection_path in connections {
            let settings =
                wifi_settings::get_connection_settings(self.conn.clone(), connection_path.clone())
                    .await?;
            if let Some(settings) = settings {
                // A matching connection could be found. Replace the settings with new ones and store to disk
                if settings.seen_bssids.contains(hw) {
                    return Ok(Some((connection_path, settings)));
                }
            }
        }
        return Ok(None);
    }

    /// Returns the dbus network manager api connection path and the connection_id as tuple.
    pub async fn find_connection_by_ssid(&self, ssid: &SSID) -> Result<Option<(dbus::Path<'_>,WiFiConnectionSettings)>, CaptivePortalError> {
        let connections = {
            use connections::Settings;
            let p = nonblock::Proxy::new(NM_BUSNAME, NM_SETTINGS_PATH, self.conn.clone());
            p.connections().await?
        };
        for connection_path in connections {
            let settings =
                wifi_settings::get_connection_settings(self.conn.clone(), connection_path.clone())
                    .await?;
            if let Some(settings) = settings {
                // A matching connection could be found. Replace the settings with new ones and store to disk
                if &settings.ssid == ssid {
                    return Ok(Some((connection_path, settings)));
                }
            }
        }
        return Ok(None);
    }

    /// Returns a tuple with network manager dbus paths on success: (connection, active_connection)
    pub async fn update_connection<'a>(
        &self,
        connection_path: dbus::Path<'a>,
        ssid: &SSID,
        old_connection: WiFiConnectionSettings,
        credentials: security::AccessPointCredentials,
    ) -> Result<(dbus::Path<'a>, dbus::Path<'_>), CaptivePortalError> {
        use connection_nm::Connection;
        let p = nonblock::Proxy::new(
            NM_BUSNAME,
            connection_path.clone(),
            self.conn.clone(),
        );
        let settings =
            wifi_settings::make_arguments_for_ap::<&'static str>(ssid, credentials,Some(old_connection))?;
        p.update2(settings, IN_MEMORY_ONLY, VariantMap::new())
            .await?;
        // Activate connection
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
        use networkmanager::NetworkManager;
        let active_path = p
            .activate_connection(
                connection_path.clone(),
                self.wifi_device_path.clone(),
                "/".into(),
            )
            .await?;
        Ok((connection_path, active_path))
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
        credentials: security::AccessPointCredentials,
        hw: Option<String>,
        overwrite_same_ssid_connection: bool,
    ) -> Result<Option<ActiveConnection>, CaptivePortalError> {
        // try to find connection, update it, activate it and return the connection path
        let active_connection = if let Some(hw) = hw {
            if let Some((connection_path, old_connection)) = self.find_connection_by_mac(&hw).await? {
                Some(self.update_connection(connection_path, &ssid, old_connection,credentials.clone())
                    .await?)
            } else {
                None
            }
        } else if overwrite_same_ssid_connection {
            if let Some((connection_path, old_connection)) = self.find_connection_by_ssid(&ssid).await? {
                Some(self.update_connection(connection_path, &ssid, old_connection,credentials.clone())
                    .await?)
            } else {
                None
            }
        } else {
            None
        };

        // If not found: Create and activate a new connection
        let (connection_path, active_connection) =
            if let Some(active_connection) = active_connection {
                active_connection
            } else {
                let settings = wifi_settings::make_arguments_for_ap(&ssid, credentials,None)?;
                let options = wifi_settings::make_options_for_ap();

                // Create connection
                use networkmanager::NetworkManager;
                let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
                let (conn_path, active_connection, _) = p
                    .add_and_activate_connection2(
                        settings,
                        self.wifi_device_path.clone(),
                        "/".into(),
                        options,
                    )
                    .await?;
                (conn_path, active_connection)
            };

        // Wait up to 5 seconds while in Deactivated
        let state = wait_for_active_connection_state(
            self,
            connectivity::ConnectionState::Deactivated,
            active_connection.clone(),
            Duration::from_secs(5),
            true,
        ).await?;
        dbg!(state);

        {
            use device::Device;
            let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
            dbg!(connectivity::DeviceState::from(p.state().await?));
        }

        // Wait up to 30 seconds while in Activating
        let state = wait_for_active_connection_state(
            self,
            connectivity::ConnectionState::Activating,
            active_connection.clone(),
            Duration::from_secs(30),
            true,
        ).await?;
        dbg!(state);

        // Remove connection if not successful. Store it permanently if successful
        if state == connectivity::ConnectionState::Activated {
            use connection_nm::Connection;
            let p = nonblock::Proxy::new(NM_BUSNAME, connection_path.clone(), self.conn.clone());

            // Settings: Provide an empty array, to use the current settings.
            p.update2(
                VariantMapNested::new(),
                SAVE_TO_DISK_FLAG,
                VariantMap::new(),
            )
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
    /// Returns None if the access point is spawned by the own network card (hotspot).
    pub async fn access_point<'b, P: Into<dbus::Path<'b>>>(
        &self,
        ap_path: P,
    ) -> Result<Option<WifiConnection>, CaptivePortalError> {
        let ap_path = ap_path.into();
        let security = security::get_access_point_security(self.conn.clone(), &ap_path)
            .await?
            .as_str();
        let access_point_data = nonblock::Proxy::new(NM_BUSNAME, ap_path, self.conn.clone());
        use access_point::AccessPoint;
        let hw = access_point_data.hw_address().await?;
        let ssid = String::from_utf8(access_point_data.ssid().await?)?;

        if hw == self.hw {
            return Ok(None);
        }

        let wifi_connection = WifiConnection {
            ssid,
            hw,
            security,
            strength: access_point_data.strength().await?,
            frequency: access_point_data.frequency().await?,
        };
        info!("ap {:?}", &wifi_connection);
        Ok(Some(wifi_connection))
    }

    /// Return all known access points of the associated wifi device.
    /// The list might not be up to date and can be refreshed with a call to [`scan_networks`].
    ///
    /// ## Arguments
    /// * find_all: Perform a full scan. This may take up to a minute.
    pub async fn list_access_points(&self, find_all: bool) -> Result<Vec<WifiConnection>, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());

        let access_point_paths = {
            use device::DeviceWireless;
            if find_all {
                p.get_all_access_points().await?
            } else {
                p.get_access_points().await?
            }
        };

        let mut connections = Vec::with_capacity(access_point_paths.len());
        for ap_path in access_point_paths {
            if let Some(ap) = self.access_point(ap_path).await? {
                connections.push(ap);
            }
        }

        Ok(connections)
    }
}
