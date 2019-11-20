//! This module contains connectivity and state related types. This includes
//! network manager state as well as connection and device state.

use dbus::{nonblock, Path};
use dbus::arg::RefArg;
use dbus::message::SignalArgs;
use futures_util::StreamExt;
use hyper::client::connect::dns::{GaiResolver, Resolve, Name};
use std::net::{Shutdown, SocketAddr};
use std::str::FromStr;
use tokio::net::TcpStream;
use tokio::future::FutureExt;
use tokio::stream::StreamExt as TokioStreamExt;

use crate::network_backend::{NetworkBackend, NM_BUSNAME};
use crate::network_interface::{Connectivity, NetworkManagerState};
use crate::CaptivePortalError;
use crate::utils::prop_stream;

impl From<&str> for NetworkManagerState {
    fn from(state: &str) -> Self {
        match state {
            "roaming" => NetworkManagerState::Asleep,
            "disconnected" => NetworkManagerState::Disconnected,
            "disconnecting" => NetworkManagerState::Disconnecting,
            "connecting" => NetworkManagerState::Connecting,
            "connected" => NetworkManagerState::Connected,
            _ => {
                warn!("Undefined Network Manager state: {}", state);
                NetworkManagerState::Unknown
            }
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

        let stream = prop_stream(&self.wifi_device_path, self.conn.clone()).await?;
        pin_utils::pin_mut!(stream);
        let mut stream = stream; // Idea IDE Workaround

        while let Some((value, _path)) = stream.next().await {
            if let Some(value) = value.changed_properties.get("State") {
                info!(
                    "Connection state changed: {}",
                    value
                        .as_str()
                        .ok_or(CaptivePortalError::Generic("Expected string as state"))?
                );
            }
            if let Some(value) = value.changed_properties.get("ConnectedNetwork") {
                info!(
                    "Connection network changed: {}",
                    value
                        .as_str()
                        .ok_or(CaptivePortalError::Generic("Expected string as network"))?
                );
            }
        }

        Ok(())
    }

    pub async fn wait_until_state(
        &self,
        expected_state: NetworkManagerState,
        timeout: Option<std::time::Duration>,
        negate_condition: bool,
    ) -> Result<NetworkManagerState, CaptivePortalError> {
        // Get current state: This also makes sure we are in station mode
        let mut state = self.state().await?;
        if (state == expected_state) ^ negate_condition {
            return Ok(state);
        }

        // Wait for connected state
        let stream = prop_stream(&self.wifi_device_path, self.conn.clone()).await?;

        let mut stream = stream; // Idea IDE Workaround
        loop {
            let v = match timeout {
                Some(timeout) => stream.next().timeout(timeout).await,
                None => Ok(stream.next().await)
            };
            match v {
                Ok(Some((value, _path))) => {
                    if let Some(value) = value.changed_properties.get("State") {
                        if let Some(state_str) = value.as_str() {
                            state = NetworkManagerState::from(state_str);
                            if (state == expected_state) ^ negate_condition {
                                return Ok(state);
                            }
                        }
                    }
                }
                _ => break,
            }
        }

        Ok(state)
    }

    /// The returned future resolves when either the timeout expired or connectivity has been established.
    /// iwd does not offer a native way to report internet connectivity.
    ///
    /// This method will try to dns resolve www.google.com and establish a tcp connection to determine
    /// full internet connectivity.
    pub async fn wait_for_connectivity(
        &self,
        internet_connectivity: bool,
        timeout: std::time::Duration,
    ) -> Result<Connectivity, CaptivePortalError> {
        let state = self.wait_until_state(NetworkManagerState::Connected, Some(timeout), false).await?;
        if state != NetworkManagerState::Connected {
            return Ok(Connectivity::None);
        }

        // Require internet connection?
        if !internet_connectivity {
            return Ok(Connectivity::Limited);
        }

        /// Resolve dns: This may be cached however and cannot be used as connectivity indicator
        let r = GaiResolver::new()
            .resolve(Name::from_str("www.google.com").unwrap())
            .timeout(timeout)
            .await;
        let mut r = match r {
            Ok(Ok(v)) => v,
            _ => return Ok(Connectivity::Limited),
        };
        /// Take first IPv4 of the dns response
        let r = r.find(|p| p.is_ipv4());
        let r = match r {
            Some(v) => v,
            None => return Ok(Connectivity::Limited),
        };
        /// Try to establish a TCP connection
        let r = TcpStream::connect(SocketAddr::new(r, 80)).timeout(timeout).await;
        match r {
            Ok(Ok(v)) => {
                let _ = v.shutdown(Shutdown::Both);
                Ok(Connectivity::Full)
            }
            _ => Ok(Connectivity::Limited),
        }
    }
}
