use enumflags2::BitFlags;
use lazy_static::lazy_static;

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

// State types for convenience
lazy_static! {
pub static ref NETWORK_MANAGER_STATE_NOT_CONNECTED: BitFlags<NetworkManagerState> = NetworkManagerState::Unknown | NetworkManagerState::Asleep | NetworkManagerState::Disconnected;
pub static ref NETWORK_MANAGER_STATE_CONNECTED: BitFlags<NetworkManagerState> = NetworkManagerState::ConnectedGlobal | NetworkManagerState::ConnectedLocal | NetworkManagerState::ConnectedSite;
pub static ref NETWORK_MANAGER_STATE_TEMP: BitFlags<NetworkManagerState> = NetworkManagerState::Connecting | NetworkManagerState::Disconnecting;
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

impl From<i64> for DeviceState {
    fn from(state: i64) -> Self {
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
    Unknown=0,
    Ethernet=1,
    WiFi=2,
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
