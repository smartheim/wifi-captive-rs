//! Hotspot related functionality
use dbus::nonblock;
use std::net::Ipv4Addr;

use super::wifi_settings::{self, VariantMap, VariantMapNested, WifiConnectionMode};
use super::{
    NetworkBackend, HOTSPOT_UUID, IN_MEMORY_ONLY, NM_BUSNAME, NM_PATH, NM_SETTINGS_PATH,
    VOLATILE_FLAG,
};
use crate::dbus_tokio::SignalStream;
use crate::network_interface::{ActiveConnection, ConnectionState, SSID};
use crate::CaptivePortalError;
use dbus::message::SignalArgs;
use futures_util::StreamExt;

impl NetworkBackend {
    /// The hotspot that is created by this service has a unique id.
    /// This method will search connections for this id and delete the respective connection.
    ///
    /// This is necessary so that network manager does not try to auto connect to the hotspot
    /// connection if nothing else can be found.
    async fn hotspot_remove_existing(&self) -> Result<(), CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_SETTINGS_PATH, self.conn.clone());
        use super::generated::connections::Settings;
        match p.get_connection_by_uuid(HOTSPOT_UUID).await {
            Ok(connection_path) => {
                info!("Deleting old hotspot configuration {}", connection_path);
                let p = nonblock::Proxy::new(NM_BUSNAME, connection_path, self.conn.clone());
                use super::generated::connection_nm::Connection;
                p.delete().await?;
            },
            Err(_) => {},
        }
        Ok(())
    }

    /// Deactivate all hotspot connections
    pub async fn deactivate_hotspots(&self) -> Result<(), CaptivePortalError> {
        self.hotspot_remove_existing().await?;

        use super::generated::networkmanager::NetworkManager;
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());

        let connections = p.active_connections().await?;
        info!(
            "Scan {} connections for hotspot connections ...",
            connections.len()
        );

        for connection_path in connections {
            let settings =
                wifi_settings::get_connection_settings(self.conn.clone(), connection_path.clone())
                    .await;
            match settings {
                Ok(Some(settings)) => {
                    if settings.mode == WifiConnectionMode::AP {
                        info!(
                            "disable hotspot connection {} {}",
                            settings.uuid, settings.ssid
                        );
                        p.deactivate_connection(connection_path).await?;
                    }
                },
                Err(e) => {
                    warn!("{}", e);
                },
                _ => {},
            }
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
        self.hotspot_remove_existing().await?;

        info!("Configuring hotspot ...");
        let connection_path = {
            // add connection
            let settings = wifi_settings::make_arguments_for_sta(
                ssid,
                password,
                address,
                &self.interface_name,
                HOTSPOT_UUID,
            )?;
            let p = nonblock::Proxy::new(NM_BUSNAME, NM_SETTINGS_PATH, self.conn.clone());
            use super::generated::connections::Settings;
            // We want the dbus nm api AddConnection2 here, but that's not yet available everywhere as of Oct 2019.
            // Instead we first add the connection and then use Update2.
            let connection_path = p.add_connection(settings).await?;

            use super::generated::connection_nm::Connection;
            let p = nonblock::Proxy::new(NM_BUSNAME, connection_path.clone(), self.conn.clone());
            // Do not set volatile here! volatile would immediately delete the connection.
            // Settings: Provide an empty array, to use the current settings.
            p.update2(VariantMapNested::new(), IN_MEMORY_ONLY, VariantMap::new())
                .await?;
            connection_path
        };

        info!("Starting hotspot ...");
        let active_connection = {
            let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
            use super::generated::networkmanager::NetworkManager;
            p.activate_connection(
                connection_path.clone(),
                self.wifi_device_path.clone(),
                dbus::Path::new("/")?,
            )
            .await?
        };

        {
            let p = nonblock::Proxy::new(NM_BUSNAME, active_connection.clone(), self.conn.clone());
            use super::generated::connection_active::ConnectionActive;
            let state: ConnectionState = p.state().await?.into();
            info!("Wait for hotspot to settle ... {:?}", state);
        }

        let state_after_wait = self
            .wait_for_active_connection_state(
                ConnectionState::Activated,
                active_connection.clone(),
                std::time::Duration::from_millis(5000),
                false,
            )
            .await?;

        if state_after_wait != ConnectionState::Activated {
            info!("Hotspot starting failed with state {:?}", state_after_wait);
            return Err(CaptivePortalError::hotspot_failed());
        }

        {
            // Make connection "volatile". Can only be done on active connections.
            use super::generated::connection_nm::Connection;
            let p = nonblock::Proxy::new(NM_BUSNAME, connection_path.clone(), self.conn.clone());

            // Settings: Provide an empty array, to use the current settings.
            if let Err(e) = p
                .update2(VariantMapNested::new(), VOLATILE_FLAG, VariantMap::new())
                .await
            {
                warn!("Failed to make hotspot volatile: {}", e);
            }
        }

        Ok(ActiveConnection {
            connection_path: connection_path.into_static(),
            active_connection_path: active_connection.into_static(),
            state: state_after_wait,
        })
    }

    pub async fn on_hotspot_stopped(&self, path: dbus::Path<'_>) -> Result<(), CaptivePortalError> {
        use super::connection_active::ConnectionActiveStateChanged as StateChanged;

        let rule = StateChanged::match_rule(None, Some(&path)).static_clone();
        let mut stream: SignalStream<StateChanged, u32> =
            SignalStream::new(self.conn.clone(), rule, Box::new(|v: StateChanged| v.state)).await?;
        for (state, _path) in stream.next().await {
            if ConnectionState::from(state) != ConnectionState::Activated {
                return Ok(());
            }
        }
        Ok(())
    }
}
