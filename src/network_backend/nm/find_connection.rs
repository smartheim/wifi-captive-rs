//! # Find a connection on some criteria; Update connection
//! Implementation detail of the network manager implementation

use dbus::nonblock;

use super::wifi_settings::{self, VariantMap, WiFiConnectionSettings};
use crate::network_backend::{
    NetworkBackend, IN_MEMORY_ONLY, NM_BUSNAME, NM_PATH, NM_SETTINGS_PATH,
};
use crate::network_interface::{AccessPointCredentials, SSID};
use crate::CaptivePortalError;

impl NetworkBackend {
    /// Returns the dbus network manager api connection path and old connection settings as tuple.
    pub(crate) async fn find_connection_by_mac(
        &self,
        hw: &String,
    ) -> Result<Option<(dbus::Path<'_>, WiFiConnectionSettings)>, CaptivePortalError> {
        let connections = {
            use super::generated::connections::Settings;
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
    pub(crate) async fn find_connection_by_ssid(
        &self,
        ssid: &SSID,
    ) -> Result<Option<(dbus::Path<'_>, WiFiConnectionSettings)>, CaptivePortalError> {
        let connections = {
            use super::generated::connections::Settings;
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
    pub(crate) async fn update_connection<'a>(
        &self,
        connection_path: dbus::Path<'a>,
        ssid: &SSID,
        old_connection: WiFiConnectionSettings,
        credentials: AccessPointCredentials,
    ) -> Result<(dbus::Path<'a>, dbus::Path<'_>), CaptivePortalError> {
        use super::generated::connection_nm::Connection;
        let p = nonblock::Proxy::new(NM_BUSNAME, connection_path.clone(), self.conn.clone());
        let settings = wifi_settings::make_arguments_for_ap::<&'static str>(
            ssid,
            credentials,
            Some(old_connection),
        )?;
        p.update2(settings, IN_MEMORY_ONLY, VariantMap::new())
            .await?;
        // Activate connection
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
        use super::generated::networkmanager::NetworkManager;
        let active_path = p
            .activate_connection(
                connection_path.clone(),
                self.wifi_device_path.clone(),
                "/".into(),
            )
            .await?;
        Ok((connection_path, active_path))
    }
}
