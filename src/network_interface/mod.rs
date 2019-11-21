//! # Generic types, traits and methods for network interfaces
//! Find implementations in [`network_backend`]
mod connection;
mod signal_stream;

pub mod dbus_tokio {
    pub use super::connection::*;
    pub use super::signal_stream::SignalStream;
}

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
    pub access_point: WifiConnection,
    pub event: WifiConnectionEventType,
}

#[derive(Serialize)]
pub struct WifiConnections(pub Vec<WifiConnection>);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Unknown,
    Activating,
    Activated,
    Deactivating,
    Deactivated,
}

/// The connection state.
/// This is mapped to iwd's internal "connected", "disconnected", "connecting", "disconnecting", "roaming" states.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum NetworkManagerState {
    /// Networking state is unknown. This indicates a daemon error that makes it unable to reasonably assess the state.
    Unknown,
    /// Networking is not enabled, the system is being suspended or resumed from suspend.
    Asleep,
    /// There is no active network connection.
    Disconnected,
    /// Network connections are being cleaned up. The applications should tear down their network sessions.
    Disconnecting,
    /// A network connection is being started
    Connecting,
    /// There is only site-wide IPv4 and/or IPv6 connectivity. This means a default route is available,
    /// but the Internet connectivity check did not succeed.
    ///
    /// Network manager checks for connectivity on its own.
    /// The connman backend tries to perform a dns resolving and establish a tcp connection
    /// to prove connectivity.
    ConnectedLimited,
    /// There is global IPv4 and/or IPv6 Internet connectivity.
    /// This means the Internet connectivity check succeeded.
    Connected,
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

/// The encryption used on a given WiFi connection or a requested encryption
/// for a new connection. Nowadays it can be expected that every WiFi adapter
/// is capable of WPA2 and WPA Enterprise.
pub enum Security {
    /// An open network
    NONE,
    // Do not use WEP for new connections! Do not connect to an access point using WEP!
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
            _ => Err(CaptivePortalError::Generic(format!(
                "Expected an encryption mode. Got: {}",
                &mode
            ))),
        }
    }
}

/// Different encryption mechanisms require different sets of credentials.
#[derive(Debug, Clone)]
pub enum AccessPointCredentials {
    None,
    Wep { passphrase: String },
    Wpa { passphrase: String },
    Enterprise { identity: String, passphrase: String },
}

/// Converts a set of credentials into the [`AccessPointCredentials`] type.
pub fn credentials_from_data(
    passphrase: String,
    identity: Option<String>,
    mode: Security,
) -> Result<AccessPointCredentials, CaptivePortalError> {
    match mode {
        Security::ENTERPRISE => Ok(AccessPointCredentials::Enterprise {
            identity: identity.ok_or(CaptivePortalError::NoSharedKeyProvided)?,
            passphrase,
        }),
        Security::WPA | Security::WPA2 => Ok(AccessPointCredentials::Wpa { passphrase }),
        Security::WEP => Ok(AccessPointCredentials::Wep { passphrase }),
        Security::NONE => Ok(AccessPointCredentials::None),
    }
}
