#[derive(Clone, Debug, PartialEq)]
pub enum NetworkManagerState {
    Unknown,
    Asleep,
    Disconnected,
    Disconnecting,
    Connecting,
    ConnectedLocal,
    ConnectedSite,
    ConnectedGlobal,
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
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Connectivity {
    Unknown,
    None,
    Portal,
    Limited,
    Full,
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
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{connection::list_connections, device::get_devices, NetworkManager};
    use super::*;

    #[test]
    fn test_get_connections() {
        let manager = NetworkManager::new();
        let connections = list_connections(&manager).unwrap();
        assert!(connections.len() > 0);
    }

    #[test]
    fn test_get_devices() {
        let manager = NetworkManager::new();
        let devices = get_devices(&manager).unwrap();
        assert!(devices.len() > 0);
    }

    #[test]
    fn test_get_connectivity() {
        let manager = NetworkManager::new();
        let connectivity = manager.check_connectivity().unwrap();
        assert_eq!(connectivity, Connectivity::Full);
    }
}
