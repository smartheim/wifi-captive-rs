pub mod dbus_tokio;

use crate::CaptivePortalError;
use core::fmt;
use serde::Serialize;
use std::convert::TryFrom;

/// A wifi SSID
/// According to last standard 802.11-2012 (Section 6.3.11.2.2),
/// a SSID  can be 0-32 octets with an unspecified or UTF8 encoding.
pub type SSID = String;

#[derive(Serialize, Clone, Debug)]
pub struct WifiConnection {
    pub ssid: SSID,
    /// The unique hw address of the access point
    pub hw: String,
    // The wifi mode
    pub security: &'static str,
    // The signal strength
    pub strength: u8,
    // The frequency
    pub frequency: u32,
    // True if this is spawned by the current device
    pub is_own: bool,
}

#[derive(Serialize, Debug, Copy, Clone)]
pub enum WifiConnectionEventType {
    Added,
    Removed,
}

impl fmt::Display for WifiConnectionEventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Serialize)]
pub struct WifiConnectionEvent {
    pub connection: WifiConnection,
    pub event: WifiConnectionEventType,
}

#[derive(Serialize)]
pub struct WifiConnections(pub Vec<WifiConnection>);

#[derive(Debug, Clone)]
pub enum AccessPointCredentials {
    None,
    Wep {
        passphrase: String,
    },
    Wpa {
        passphrase: String,
    },
    Enterprise {
        identity: String,
        passphrase: String,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Unknown,
    Activating,
    Activated,
    Deactivating,
    Deactivated,
}

/// The connection state.
/// iwd: "connected", "disconnected", "connecting", "disconnecting", "roaming"
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum NetworkManagerState {
    Unknown,
    Asleep,
    Disconnected,
    Disconnecting,
    Connecting,
    Connected,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Connectivity {
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

impl From<NetworkManagerState> for Connectivity {
    fn from(state: NetworkManagerState) -> Self {
        match state {
            NetworkManagerState::Connected => Connectivity::Limited,
            _ => Connectivity::None,
        }
    }
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
            },
        }
    }
}

pub enum Security {
    NONE,
    WEP,
    WPA,
    WPA2,
    ENTERPRISE,
}

impl Security {
    pub fn as_str(&self) -> &'static str {
        match self {
            Security::NONE => "none",
            Security::ENTERPRISE => "enterprise",
            Security::WEP => "wep",
            Security::WPA | Security::WPA2 => "wpa",
        }
    }
}

impl TryFrom<String> for Security {
    type Error = CaptivePortalError;

    fn try_from(mode: String) -> Result<Self, Self::Error> {
        match &mode[..] {
            "enterprise" => Ok(Security::ENTERPRISE),
            "wpa" => Ok(Security::WPA),
            "wpa2" => Ok(Security::WPA2),
            "wep" => Ok(Security::WEP),
            "open" | "" => Ok(Security::NONE),
            _ => Err(CaptivePortalError::GenericO(format!(
                "Expected an encryption mode. Got: {}",
                &mode
            ))),
        }
    }
}

/// Represents an active connection.
/// In iwd this is called "known network".
///
/// There can be multiple active connections if multiple network devices (wired, wireless cards)
/// are present.
pub struct ActiveConnection {
    /// The dbus path to the underlying connection. In iwd this is called "network".
    pub connection_path: dbus::Path<'static>,
    /// The dbus path to the active connection. In iwd this is called "known network".
    pub active_connection_path: dbus::Path<'static>,
    pub state: ConnectionState,
}

// Converts a set of credentials into the [`AccessPointCredentials`] type.
pub fn credentials_from_data(
    passphrase: Option<String>,
    identity: Option<String>,
    mode: Security,
) -> Result<AccessPointCredentials, CaptivePortalError> {
    match mode {
        Security::ENTERPRISE => Ok(AccessPointCredentials::Enterprise {
            identity: identity.ok_or(CaptivePortalError::no_shared_key())?,
            passphrase: passphrase.ok_or(CaptivePortalError::no_shared_key())?,
        }),
        Security::WPA | Security::WPA2 => Ok(AccessPointCredentials::Wpa {
            passphrase: passphrase.ok_or(CaptivePortalError::no_shared_key())?,
        }),
        Security::WEP => Ok(AccessPointCredentials::Wep {
            passphrase: passphrase.ok_or(CaptivePortalError::no_shared_key())?,
        }),
        Security::NONE => Ok(AccessPointCredentials::None),
    }
}
