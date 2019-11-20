//! This module contains connectivity and state related types. This includes
//! network manager state as well as connection and device state.

use futures_util::stream::StreamExt;
use tokio::future::FutureExt;

use super::NetworkBackend;
use super::NM_BUSNAME;
use super::NM_PATH;
use crate::dbus_tokio::SignalStream;
use crate::network_interface::{ConnectionState, Connectivity, NetworkManagerState};
use crate::CaptivePortalError;
use dbus::message::SignalArgs;
use dbus::nonblock;

impl From<u32> for NetworkManagerState {
    fn from(state: u32) -> Self {
        match state {
            0 => NetworkManagerState::Unknown,
            10 => NetworkManagerState::Asleep,
            20 => NetworkManagerState::Disconnected,
            30 => NetworkManagerState::Disconnecting,
            40 => NetworkManagerState::Connecting,
            50 => NetworkManagerState::Connected,
            60 => NetworkManagerState::Connected,
            70 => NetworkManagerState::Connected,
            _ => {
                warn!("Undefined Network Manager state: {}", state);
                NetworkManagerState::Unknown
            }
        }
    }
}

impl From<u32> for Connectivity {
    fn from(state: u32) -> Self {
        match state {
            0 => Connectivity::Unknown,
            1 => Connectivity::None,
            2 => Connectivity::Portal,
            3 => Connectivity::Limited,
            4 => Connectivity::Full,
            _ => {
                warn!("Undefined connectivity state: {}", state);
                Connectivity::Unknown
            }
        }
    }
}

impl NetworkBackend {
    /// Continuously print connection state changes
    #[allow(dead_code)]
    pub async fn print_connection_changes(&self) -> Result<(), CaptivePortalError> {
        use super::connection_active::ConnectionActiveStateChanged as ConnectionActiveChanged;

        let rule = ConnectionActiveChanged::match_rule(None, None).static_clone();
        let stream: SignalStream<ConnectionActiveChanged, ConnectionActiveChanged> =
            SignalStream::new(self.conn.clone(), rule, Box::new(|v| v)).await?;
        pin_utils::pin_mut!(stream);
        let mut stream = stream; // Idea IDE Workaround

        while let Some((value, path)) = stream.next().await {
            info!(
                "Connection state changed: {:?} {} on {}",
                ConnectionState::from(value.state),
                value.reason,
                path
            );
        }

        Ok(())
    }

    pub async fn wait_until_state(
        &self,
        expected_state: NetworkManagerState,
        timeout: Option<std::time::Duration>,
        negate_condition: bool,
    ) -> Result<NetworkManagerState, CaptivePortalError> {
        use super::networkmanager::NetworkManagerStateChanged as StateChanged;

        let state = self.state().await?;
        if (expected_state == state) ^ negate_condition {
            return Ok(state);
        }

        let rule = StateChanged::match_rule(None, None).static_clone();
        let stream: SignalStream<StateChanged, StateChanged> =
            SignalStream::new(self.conn.clone(), rule, Box::new(|v| v)).await?;
        pin_utils::pin_mut!(stream);
        let mut stream = stream; // Idea IDE Workaround

        match timeout {
            Some(timeout) => {
                while let Ok(state_change) = stream.next().timeout(timeout).await {
                    if let Some((value, _path)) = state_change {
                        let state = NetworkManagerState::from(value.state);
                        if (expected_state == state) ^ negate_condition {
                            return Ok(state);
                        }
                    }
                }
            }
            None => {
                while let Some((value, _path)) = stream.next().await {
                    let state = NetworkManagerState::from(value.state);
                    if (expected_state == state) ^ negate_condition {
                        return Ok(state);
                    }
                }
            }
        }

        Ok(NetworkManagerState::Unknown)
    }

    /// The returned future resolves when either the timeout expired or state of the
    /// **active** connection (eg /org/freedesktop/NetworkManager/ActiveConnection/12) is the expected state
    /// or changes into the expected state.
    pub async fn wait_for_active_connection_state(
        &self,
        expected_state: ConnectionState,
        path: dbus::Path<'_>,
        timeout: std::time::Duration,
        negate: bool,
    ) -> Result<ConnectionState, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, path, self.conn.clone());

        use super::connection_active::ConnectionActive;
        let state: ConnectionState = p.state().await?.into();
        if (state == expected_state) ^ negate {
            return Ok(state);
        }

        use super::connection_active::ConnectionActiveStateChanged as StateChanged;

        let rule = StateChanged::match_rule(None, None).static_clone();
        let stream: SignalStream<StateChanged, u32> =
            SignalStream::new(self.conn.clone(), rule, Box::new(|v: StateChanged| v.state)).await?;
        pin_utils::pin_mut!(stream);
        let mut stream = stream; // Idea IDE Workaround

        while let Ok(state_change) = stream.next().timeout(timeout).await {
            if let Some((state, _path)) = state_change {
                let state = ConnectionState::from(state);
                if (state == expected_state) ^ negate {
                    return Ok(state);
                }
            }
        }

        let state: ConnectionState = p.state().await?.into();
        Ok(state)
    }

    /// The returned future resolves when either the timeout expired or state of the
    /// **active** connection (eg /org/freedesktop/NetworkManager/ActiveConnection/12) is the expected state
    /// or changes into the expected state.
    pub async fn wait_for_connectivity(
        &self,
        internet_connectivity: bool,
        timeout: std::time::Duration,
    ) -> Result<Connectivity, CaptivePortalError> {
        let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, self.conn.clone());
        use super::networkmanager::NetworkManager;

        let state = Connectivity::from(p.connectivity().await?);
        if state == Connectivity::Full || (state == Connectivity::Limited && !internet_connectivity) {
            return Ok(state);
        }

        use super::networkmanager::NetworkManagerStateChanged as StateChanged;

        let rule = StateChanged::match_rule(None, None).static_clone();
        let stream: SignalStream<StateChanged, u32> =
            SignalStream::new(self.conn.clone(), rule, Box::new(|v: StateChanged| v.state)).await?;
        pin_utils::pin_mut!(stream);
        let mut stream = stream; // Idea IDE Workaround

        while let Ok(state_change) = stream.next().timeout(timeout).await {
            // Whenever network managers state change, request the current connectivity
            if let Some((_state, _path)) = state_change {
                let state: Connectivity = p.connectivity().await?.into();
                if state == Connectivity::Full || (state == Connectivity::Limited && !internet_connectivity) {
                    return Ok(state);
                }
            }
        }

        let state: Connectivity = p.connectivity().await?.into();
        Ok(state)
    }

    pub async fn enable_auto_connect(&self) {
        use super::device::Device;
        let p = nonblock::Proxy::new(NM_BUSNAME, &self.wifi_device_path, self.conn.clone());
        if let Err(e) = p.set_autoconnect(true).await {
            warn!(
                "Failed to enable autoconnect for {}: {}",
                self.interface_name, e
            );
        }
    }
}
