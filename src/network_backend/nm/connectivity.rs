//! This module contains connectivity and state related types. This includes
//! network manager state as well as connection and device state.

use futures_util::stream::StreamExt;
use tokio::time::timeout;

use super::NetworkBackend;
use super::NM_BUSNAME;
use crate::dbus_tokio::SignalStream;
use crate::network_backend::NM_PATH;
use crate::network_interface::{ConnectionState, NetworkManagerState};
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
            // NM_STATE_CONNECTED_LOCAL
            // There is only local IPv4 and/or IPv6 connectivity, but no default route to access the Internet.
            50 => NetworkManagerState::Disconnected,
            // NM_STATE_CONNECTED_SITE
            60 => NetworkManagerState::ConnectedLimited,
            70 => NetworkManagerState::Connected,
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
        use super::connection_active::ConnectionActiveStateChanged as ConnectionActiveChanged;

        let mut stream =
            SignalStream::<ConnectionActiveChanged>::prop_new(&self.wifi_device_path, self.conn.clone()).await?;
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

    /// Continuously print connection state changes
    #[allow(dead_code)]
    pub async fn print_connectivity_changes(&self) -> Result<(), CaptivePortalError> {
        use super::networkmanager::NetworkManagerStateChanged as StateChanged;

        let state = self.state().await?;
        info!("Connectivity state: {:?}", state);

        let mut stream = SignalStream::<StateChanged>::prop_new(&NM_PATH.to_owned().into(), self.conn.clone()).await?;
        while let Some((value, _path)) = stream.next().await {
            let state = NetworkManagerState::from(value.state);
            info!("Connectivity state changed: {:?}", state);
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
        timeout_value: std::time::Duration,
        condition: F,
    ) -> Result<NetworkManagerState, CaptivePortalError>
        where
            F: Fn(NetworkManagerState) -> bool,
    {
        use super::networkmanager::NetworkManagerStateChanged as StateChanged;

        let mut state = self.state().await?;
        if condition(state) {
            return Ok(state);
        }

        let mut stream = SignalStream::<StateChanged>::prop_new(&NM_PATH.to_owned().into(), self.conn.clone())
            .await?;
        while let Ok(Some((value, _path))) = timeout(timeout_value, stream.next()).await {
            state = NetworkManagerState::from(value.state);
            if condition(state) {
                return Ok(state);
            }
        }

        if condition(state) {
            Ok(state)
        } else {
            Err(CaptivePortalError::NotRequiredConnectivity(state))
        }
    }

    /// The returned future resolves when either the timeout expired or state of the
    /// **active** connection (eg /org/freedesktop/NetworkManager/ActiveConnection/12) is the expected state
    /// or changes into the expected state.
    pub async fn wait_for_active_connection_state(
        &self,
        expected_state: ConnectionState,
        path: dbus::Path<'_>,
        timeout_value: std::time::Duration,
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
        let stream: SignalStream<StateChanged> = SignalStream::new(self.conn.clone(), rule).await?;
        pin_utils::pin_mut!(stream);
        let mut stream = stream; // Idea IDE Workaround

        while let Ok(Some((state, _path))) = timeout(timeout_value, stream.next()).await {
            let state = ConnectionState::from(state.state);
            if (state == expected_state) ^ negate {
                return Ok(state);
            }
        }

        let state: ConnectionState = p.state().await?.into();
        Ok(state)
    }

    pub async fn enable_auto_connect(&self) {
        use super::device::Device;
        let p = nonblock::Proxy::new(NM_BUSNAME, &self.wifi_device_path, self.conn.clone());
        if let Err(e) = p.set_autoconnect(true).await {
            warn!("Failed to enable autoconnect for {}: {}", self.interface_name, e);
        }
    }
}
