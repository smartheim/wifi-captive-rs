use super::super::{NetworkManager, NetworkManagerError, NM_ACTIVE_INTERFACE};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ConnectionState {
    Unknown = 0,
    Activating = 1,
    Activated = 2,
    Deactivating = 3,
    Deactivated = 4,
}

impl From<i64> for ConnectionState {
    fn from(state: i64) -> Self {
        match state {
            0 => ConnectionState::Unknown,
            1 => ConnectionState::Activating,
            2 => ConnectionState::Activated,
            3 => ConnectionState::Deactivating,
            4 => ConnectionState::Deactivated,
            _ => {
                warn!("Undefined connection state: {}", state);
                ConnectionState::Unknown
            },
        }
    }
}

pub fn get_connection_state(
    dbus_manager: NetworkManager,
    path: &str,
) -> Result<ConnectionState, NetworkManagerError> {
    let state: i64 = match dbus_manager
        .dbus
        .property(path, NM_ACTIVE_INTERFACE, "State")
    {
        Ok(state) => state,
        Err(_) => return Ok(ConnectionState::Unknown),
    };

    Ok(ConnectionState::from(state))
}
