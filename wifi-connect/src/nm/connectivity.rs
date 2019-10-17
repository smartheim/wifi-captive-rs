//! This module contains connectivity and state related types. This includes
//! network manager state as well as connection and device state.

use enumflags2::BitFlags;
use lazy_static::lazy_static;
use futures_util::stream::StreamExt;
use tokio::future::FutureExt;

use super::{
    dbus_tokio::SignalStream,
    CaptivePortalError,
    NetworkManager,
    NM_BUSNAME,
    NM_PATH,
};
use dbus::{
    nonblock,
    message::SignalArgs,
};

#[derive(BitFlags, Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum NetworkManagerState {
    Unknown = 128,
    Asleep = 1,
    Disconnected = 2,
    Disconnecting = 4,
    Connecting = 8,
    ConnectedLocal = 16,
    ConnectedSite = 32,
    ConnectedGlobal = 64,
}

impl From<u32> for NetworkManagerState {
    fn from(state: u32) -> Self {
        match state {
            0 => NetworkManagerState::Unknown,
            10 => NetworkManagerState::Asleep,
            20 => NetworkManagerState::Disconnected,
            30 => NetworkManagerState::Disconnecting,
            40 => NetworkManagerState::Connecting,
            50 => NetworkManagerState::ConnectedLocal,
            60 => NetworkManagerState::ConnectedSite,
            70 => NetworkManagerState::ConnectedGlobal,
            _ => {
                warn!("Undefined Network Manager state: {}", state);
                NetworkManagerState::Unknown
            }
        }
    }
}

#[derive(BitFlags, Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum Connectivity {
    Unknown = 128,
    None = 1,
    Portal = 2,
    Limited = 4,
    Full = 8,
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

// State types for convenience
lazy_static! {
    pub static ref NETWORK_MANAGER_STATE_NOT_CONNECTED: BitFlags<NetworkManagerState> =
        NetworkManagerState::Unknown
            | NetworkManagerState::Asleep
            | NetworkManagerState::Disconnected;
    pub static ref NETWORK_MANAGER_STATE_CONNECTED: BitFlags<NetworkManagerState> =
        NetworkManagerState::ConnectedGlobal
            | NetworkManagerState::ConnectedLocal
            | NetworkManagerState::ConnectedSite;
    pub static ref NETWORK_MANAGER_STATE_TEMP: BitFlags<NetworkManagerState> =
        NetworkManagerState::Connecting | NetworkManagerState::Disconnecting;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Unknown,
    Activating,
    Activated,
    Deactivating,
    Deactivated,
}

impl From<u32> for ConnectionState {
    fn from(state: u32) -> Self {
        match state {
            0 => ConnectionState::Unknown,
            1 => ConnectionState::Activating,
            2 => ConnectionState::Activated,
            3 => ConnectionState::Deactivating,
            4 => ConnectionState::Deactivated,
            _ => {
                warn!("Undefined connection state: {}", state);
                ConnectionState::Unknown
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DeviceState {
    Unknown,
    Unmanaged,
    Unavailable,
    Disconnected,
    Prepare,
    Config,
    NeedAuth,
    IpConfig,
    IpCheck,
    Secondaries,
    Activated,
    Deactivating,
    Failed,
}

impl From<u32> for DeviceState {
    fn from(state: u32) -> Self {
        match state {
            0 => DeviceState::Unknown,
            10 => DeviceState::Unmanaged,
            20 => DeviceState::Unavailable,
            30 => DeviceState::Disconnected,
            40 => DeviceState::Prepare,
            50 => DeviceState::Config,
            60 => DeviceState::NeedAuth,
            70 => DeviceState::IpConfig,
            80 => DeviceState::IpCheck,
            90 => DeviceState::Secondaries,
            100 => DeviceState::Activated,
            110 => DeviceState::Deactivating,
            120 => DeviceState::Failed,
            _ => {
                warn!("Undefined device state: {}", state);
                DeviceState::Unknown
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DeviceType {
    Unknown = 0,
    Ethernet = 1,
    WiFi = 2,
    Unused1,
    Unused2,
    Bt,
    OlpcMesh,
    Wimax,
    Modem,
    Infiniband,
    Bond,
    Vlan,
    Adsl,
    Bridge,
    Generic,
    Team,
    Tun,
    IpTunnel,
    Macvlan,
    Vxlan,
    Veth,
    Macsec,
    Dummy,
}

impl From<i64> for DeviceType {
    fn from(device_type: i64) -> Self {
        match device_type {
            0 => DeviceType::Unknown,
            1 => DeviceType::Ethernet,
            2 => DeviceType::WiFi,
            3 => DeviceType::Unused1,
            4 => DeviceType::Unused2,
            5 => DeviceType::Bt,
            6 => DeviceType::OlpcMesh,
            7 => DeviceType::Wimax,
            8 => DeviceType::Modem,
            9 => DeviceType::Infiniband,
            10 => DeviceType::Bond,
            11 => DeviceType::Vlan,
            12 => DeviceType::Adsl,
            13 => DeviceType::Bridge,
            14 => DeviceType::Generic,
            15 => DeviceType::Team,
            16 => DeviceType::Tun,
            17 => DeviceType::IpTunnel,
            18 => DeviceType::Macvlan,
            19 => DeviceType::Vxlan,
            20 => DeviceType::Veth,
            21 => DeviceType::Macsec,
            22 => DeviceType::Dummy,
            _ => {
                warn!("Undefined device type: {}", device_type);
                DeviceType::Unknown
            }
        }
    }
}


pub async fn on_active_connection_state_change(
    nm: &NetworkManager,
    path: dbus::Path<'_>,
) -> Result<ConnectionState, CaptivePortalError> {
    use super::connection_active::ConnectionActiveStateChanged as StateChanged;

    let rule = StateChanged::match_rule(None, Some(&path)).static_clone();
    let stream: SignalStream<StateChanged, u32> =
        SignalStream::new(nm.conn.clone(), rule, Box::new(|v: StateChanged| v.state)).await?;
    pin_utils::pin_mut!(stream);
    let mut stream = stream; // Idea IDE Workaround
    Ok(match stream.next().await {
        None => ConnectionState::Unknown,
        Some(v) => v.0.into(),
    })
}


/// Continuously print connection state changes
#[allow(dead_code)]
pub async fn print_connection_changes(nm: &NetworkManager) -> Result<(), CaptivePortalError> {
    use super::connection_active::ConnectionActiveStateChanged as ConnectionActiveChanged;

    let rule = ConnectionActiveChanged::match_rule(None, None).static_clone();
    let stream: SignalStream<ConnectionActiveChanged, ConnectionActiveChanged> =
        SignalStream::new(nm.conn.clone(), rule, Box::new(|v| v)).await?;
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
    nm: &NetworkManager,
    expected_states: BitFlags<NetworkManagerState>,
    timeout: Option<std::time::Duration>,
    negate_condition: bool,
) -> Result<NetworkManagerState, CaptivePortalError> {
    use super::networkmanager::NetworkManagerStateChanged as StateChanged;

    let state = nm.state().await?;
    if expected_states.contains(state) ^ negate_condition {
        return Ok(state);
    }

    let rule = StateChanged::match_rule(None, None).static_clone();
    let stream: SignalStream<StateChanged, StateChanged> =
        SignalStream::new(nm.conn.clone(), rule, Box::new(|v| v)).await?;
    pin_utils::pin_mut!(stream);
    let mut stream = stream; // Idea IDE Workaround

    match timeout {
        Some(timeout) => {
            while let Ok(state_change) = stream.next().timeout(timeout).await {
                if let Some((value, _path)) = state_change {
                    let state = NetworkManagerState::from(value.state);
                    if expected_states.contains(state) ^ negate_condition {
                        return Ok(state);
                    }
                }
            }
        }
        None => {
            while let Some((value, _path)) = stream.next().await {
                let state = NetworkManagerState::from(value.state);
                if expected_states.contains(state) ^ negate_condition {
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
    nm: &NetworkManager,
    expected_state: ConnectionState,
    path: dbus::Path<'_>,
    timeout: std::time::Duration,
    negate: bool,
) -> Result<ConnectionState, CaptivePortalError> {
    let p = nonblock::Proxy::new(NM_BUSNAME, path, nm.conn.clone());

    use super::connection_active::ConnectionActive;
    let state: ConnectionState = p.state().await?.into();
    if (state == expected_state) ^ negate {
        return Ok(state);
    }

    use super::connection_active::ConnectionActiveStateChanged as StateChanged;

    let rule = StateChanged::match_rule(None, None).static_clone();
    let stream: SignalStream<StateChanged, u32> =
        SignalStream::new(nm.conn.clone(), rule, Box::new(|v: StateChanged| v.state)).await?;
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
    nm: &NetworkManager,
    expected: BitFlags<Connectivity>,
    timeout: std::time::Duration,
    negate: bool,
) -> Result<Connectivity, CaptivePortalError> {
    let p = nonblock::Proxy::new(NM_BUSNAME, NM_PATH, nm.conn.clone());
    use super::networkmanager::NetworkManager;

    let state: Connectivity = p.connectivity().await?.into();
    if expected.contains(state) ^ negate {
        return Ok(state);
    }

    use super::networkmanager::NetworkManagerStateChanged as StateChanged;

    let rule = StateChanged::match_rule(None, None).static_clone();
    let stream: SignalStream<StateChanged, u32> =
        SignalStream::new(nm.conn.clone(), rule, Box::new(|v: StateChanged| v.state)).await?;
    pin_utils::pin_mut!(stream);
    let mut stream = stream; // Idea IDE Workaround

    while let Ok(state_change) = stream.next().timeout(timeout).await {
        // Whenever network managers state change, request the current connectivity
        if let Some((_state, _path)) = state_change {
            let state: Connectivity = p.connectivity().await?.into();
            if expected.contains(state) ^ negate {
                return Ok(state);
            }
        }
    }

    let state: Connectivity = p.connectivity().await?.into();
    Ok(state)
}

impl NetworkManager {
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