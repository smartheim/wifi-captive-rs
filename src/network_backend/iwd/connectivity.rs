//! This module contains connectivity and state related types. This includes
//! network manager state as well as connection and device state.

use dbus::arg::RefArg;
use dbus::message::SignalArgs;
use dbus::{nonblock, Path};
use futures_util::StreamExt;
use hyper::client::connect::dns::{GaiResolver, Name, Resolve};
use std::net::{Shutdown, SocketAddr};
use std::str::FromStr;
use tokio::future::FutureExt;
use tokio::net::TcpStream;
use tokio::stream::StreamExt as TokioStreamExt;

use crate::dbus_tokio::SignalStream;
use crate::network_backend::{NetworkBackend, NM_BUSNAME, NM_PATH};
use crate::network_interface::{Connectivity, NetworkManagerState};
use crate::CaptivePortalError;
use dbus::nonblock::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged;

impl From<&str> for NetworkManagerState {
    fn from(state: &str) -> Self {
        match state {
            "roaming" => NetworkManagerState::Connected,
            "disconnected" => NetworkManagerState::Disconnected,
            "disconnecting" => NetworkManagerState::Disconnecting,
            "connecting" => NetworkManagerState::Connecting,
            "connected" => NetworkManagerState::Connected,
            _ => {
                warn!("Undefined Network Manager state: {}", state);
                NetworkManagerState::Unknown
            },
        }
    }
}

impl NetworkBackend {
    /// Continuously print connection state changes
    #[allow(dead_code)]
    pub async fn print_connection_changes(&self) -> Result<(), CaptivePortalError> {
        use super::generated::device::NetConnmanIwdStation;

        let p = nonblock::Proxy::new(NM_BUSNAME, self.wifi_device_path.clone(), self.conn.clone());
        let conn_network: Path = p.connected_network().await?;
        info!("Connection network: {}", conn_network.to_string());

        let stream =
            SignalStream::<PropertiesPropertiesChanged>::prop_new(NM_PATH.to_owned().into(), self.conn.clone()).await?;
        pin_utils::pin_mut!(stream);
        let mut stream = stream; // Idea IDE Workaround

        while let Some((value, _path)) = stream.next().await {
            if let Some(value) = value.changed_properties.get("State") {
                info!(
                    "Connection state changed: {}",
                    value
                        .as_str()
                        .ok_or(CaptivePortalError::IwdError("Expected string as state"))?
                );
            }
            if let Some(value) = value.changed_properties.get("ConnectedNetwork") {
                info!(
                    "Connection network changed: {}",
                    value
                        .as_str()
                        .ok_or(CaptivePortalError::IwdError("Expected string as network"))?
                );
            }
        }

        Ok(())
    }

    /// The returned future resolves when either the timeout expired or state of the
    /// **active** connection (eg /org/freedesktop/NetworkManager/ActiveConnection/12) is the expected state
    /// or changes into the expected state.
    pub async fn wait_for_connectivity(
        &self,
        internet_connectivity: bool,
        timeout: std::time::Duration,
    ) -> Result<NetworkManagerState, CaptivePortalError> {
        self.connectivity_changed(timeout, |state| {
            state == NetworkManagerState::Connected
                || (state == NetworkManagerState::ConnectedLimited && !internet_connectivity)
        })
        .await
    }

    /// The returned future resolves when either the timeout expired or (internet) connectivity is lost
    pub async fn wait_for_connectivity_lost(
        &self,
        internet_connectivity: bool,
        timeout: std::time::Duration,
    ) -> Result<NetworkManagerState, CaptivePortalError> {
        self.connectivity_changed(timeout, |state| {
            state != NetworkManagerState::Connected
                && (state != NetworkManagerState::ConnectedLimited || internet_connectivity)
        })
        .await
    }

    /// Waits up to "timeout" for the network backend to report the condition given in "condition".
    async fn connectivity_changed<F>(
        &self,
        timeout: std::time::Duration,
        condition: F,
    ) -> Result<NetworkManagerState, CaptivePortalError>
    where
        F: Fn(NetworkManagerState) -> bool,
    {
        use super::networkmanager::NetworkManagerStateChanged as StateChanged;

        let mut state = self.state().await?;
        if state == NetworkManagerState::ConnectedLimited {
            state = self.test_internet_connectivity(timeout).await;
        }
        if condition(state) {
            return Ok(state);
        }

        let mut stream =
            SignalStream::<PropertiesPropertiesChanged>::prop_new(&NM_PATH.to_owned().into(), self.conn.clone())
                .await?;
        while let Ok(state_change) = stream.next().timeout(timeout).await {
            if let Some((value, _path)) = state_change {
                if let Some(value) = value.changed_properties.get("State") {
                    if let Some(state_str) = value.as_str() {
                        state = NetworkManagerState::from(state_str);
                        if state == NetworkManagerState::ConnectedLimited {
                            state = self.test_internet_connectivity(timeout).await;
                        }
                        if condition(state) {
                            return Ok(state);
                        }
                    }
                }
            }
        }

        if condition(state) {
            Ok(state)
        } else {
            Err(CaptivePortalError::NotRequiredConnectivity(state))
        }
    }

    /// Network Manager implements this internally. Connman / iwd don't. This check will try to resolve via DNS www.google.com
    /// and also tries to establish a TCP connection.
    ///
    /// This method is assumed to be called when a limited connection is already confirmed and returns
    /// [`NetworkManagerState::ConnectedLimited`] if not successful and [`NetworkManagerState::Connected`] otherwise.
    async fn test_internet_connectivity(&self, timeout: std::time::Duration) -> NetworkManagerState {
        /// Resolve dns: This may be cached however and cannot be used as connectivity indicator
        let r = GaiResolver::new()
            .resolve(Name::from_str("www.google.com").unwrap())
            .timeout(timeout)
            .await;
        let mut r = match r {
            Ok(Ok(v)) => v,
            _ => return NetworkManagerState::ConnectedLimited,
        };
        /// Take first IPv4 of the dns response
        let r = r.find(|p| p.is_ipv4());
        let r = match r {
            Some(v) => v,
            None => return NetworkManagerState::ConnectedLimited,
        };
        /// Try to establish a TCP connection
        let r = TcpStream::connect(SocketAddr::new(r, 80)).timeout(timeout).await;
        match r {
            Ok(Ok(v)) => {
                let _ = v.shutdown(Shutdown::Both);
                NetworkManagerState::Connected
            },
            _ => NetworkManagerState::ConnectedLimited,
        }
    }
}
