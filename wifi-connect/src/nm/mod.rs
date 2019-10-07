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

pub use dbus_types::*;
pub use generated::*;
pub use security::AccessPointCredentials;
pub use connectivity::{NetworkManagerState, ConnectionState};

//use dbus::channel::MatchingReceiver;
//use dbus::message::MatchRule;

use dbus::nonblock;
use dbus_tokio::connection;

use super::CaptivePortalError;
use dbus::channel::MatchingReceiver;
use dbus::nonblock::SyncConnection;
use serde::Serialize;
use std::net::Ipv4Addr;
use std::sync::Arc;
use crate::nm::wifi_settings::{get_connection_settings, WifiConnectionMode};

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

#[derive(Serialize)]
pub struct WifiConnectionEvent {
    pub connection: WifiConnection,
    pub event: NetworkManagerEvent,
}

#[derive(Serialize)]
pub struct WifiConnections(pub Vec<WifiConnection>);

pub struct NetworkManager {
    exit_handler: tokio::sync::oneshot::Sender<()>,
    conn: Arc<SyncConnection>,
    wifi_device_path: String,
    interface_name: String,
}

impl NetworkManager {
    /// Create a new connection to the network manager. This will also try to enable networking
    /// and wifi.
    pub async fn new(config: &super::config::Config) -> Result<NetworkManager, CaptivePortalError> {
        // Prepare an exit handler
        let (exit_handler, exit_receiver) = tokio::sync::oneshot::channel::<()>();

        // Connect to the D-Bus session bus (this is blocking, unfortunately).
        let (resource, conn) = connection::new_system_sync()?;

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

        let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, conn.clone());
        use networkmanager::OrgFreedesktopNetworkManager;
        if !p.networking_enabled().await? {
            p.enable(true).await?;
        }
        if !p.wireless_enabled().await? {
            p.set_wireless_enabled(true).await?;
        }

        let (wifi_device_path, interface_name) =
            utils::find_wifi_device(conn.clone(), &config.interface).await?;
        Ok(NetworkManager {
            exit_handler,
            conn,
            interface_name,
            wifi_device_path,
        })
    }

    /// Terminates this network manager dbus connection
    pub fn quit(self) {
        let _ = self.exit_handler.send(());
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
                let settings = get_connection_settings(self.conn.clone(), conn_path).await?;
                if settings.mode == WifiConnectionMode::AP && settings.ssid == ssid {
                    info!("Delete existing network manager hotspot configuration");
                    p.delete().await?;
                }
            }
        }

        let ssid_user_string = ssid.to_string();

        let settings =
            wifi_settings::make_arguments_for_sta(ssid, password, address, &self.interface_name)?;

        let p = nonblock::Proxy::new(NM_INTERFACE, &self.wifi_device_path, self.conn.clone());

        use networkmanager::OrgFreedesktopNetworkManager;
        let (conn_path, active_connection) = p
            .add_and_activate_connection(
                settings,
                dbus::Path::new(self.wifi_device_path.as_bytes())?,
                dbus::Path::new("/")?,
            )
            .await?;

        let state_after_wait = self
            .wait_for_state(
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

    /// The returned future resolves when either the timeout expired or state of the
    /// **active** connection (eg /org/freedesktop/NetworkManager/ActiveConnection/12) is the or changes into
    /// the expected state.
    pub async fn wait_for_state(
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

        use connection_active::OrgFreedesktopNetworkManagerConnectionActiveStateChanged as ConnectionActiveChanged;
        //OrgFreedesktopNetworkManagerConnectionActiveStateChanged::match_rule()

        let conn = self.conn.clone();
        //TODO simply with new dbus crate version and
        use dbus::message::SignalArgs;
        let match_rule = ConnectionActiveChanged::match_rule(None, None);
        let m = match_rule.match_str();
        //use dbus::nonblock::stdintf::org_freedesktop_dbus;
        //p.add_match(m);

        conn.start_receive(
            match_rule,
            Box::new(|h: dbus::Message, _| {
                println!("Hello happened from ConnectionActiveChanged", );
                true
            }),
        );
        //
        //        use dbus::nonblock::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged as Ppc;
        //
        //        let x = conn.add_match(Ppc::match_rule(None, None), |ppc: Ppc, _| {
        //            println("{:?}", ppc);
        //            true
        //        })?;

        //TODO

        let state: connectivity::ConnectionState = p.state().await?.into();
        Ok(state)
    }

    /// The current connectity status
    pub async fn connectivity(&self) -> Result<connectivity::Connectivity, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, self.conn.clone());
        use networkmanager::OrgFreedesktopNetworkManager;
        Ok(p.connectivity().await?.into())
    }

    /// The network manager state
    pub async fn state(&self) -> Result<connectivity::NetworkManagerState, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, self.conn.clone());
        use networkmanager::OrgFreedesktopNetworkManager;
        Ok(p.state().await?.into())
    }

    //TODO
    pub async fn connect_to(
        &self,
        ssid: SSID,
        credentials: security::AccessPointCredentials,
    ) -> Result<Vec<WifiConnection>, CaptivePortalError> {
        unimplemented!()
    }

    pub async fn list_access_points(&self) -> Result<Vec<WifiConnection>, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_INTERFACE, &self.wifi_device_path, self.conn.clone());

        let access_point_paths = {
            use device::OrgFreedesktopNetworkManagerDeviceWireless;
            p.get_all_access_points().await?
        };

        let mut connections = Vec::with_capacity(access_point_paths.len());
        for ap_path in access_point_paths {
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
            connections.push(wifi_connection);
        }

        Ok(connections)
    }
}
