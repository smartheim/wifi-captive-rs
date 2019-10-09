//! # Error and Result Type

use std::error;
use std::fmt;

/// The main error type used throughout this crate. It wraps / converts from nm_dbus_generated few other error
/// types and implements [error::Error] so that you can use it in any situation where the
/// standard error type is expected.
#[derive(Debug)]
pub enum CaptivePortalError {
    /// Generic errors are very rarely used and only used if no other error type matches
    Generic(&'static str),
    OwnedString(String),
    /// Serialisation failed
    Ser(serde_json::Error),
    Ascii(ascii::AsAsciiStrError),
    Utf8(std::str::Utf8Error),
    // Name, Message
    DBus(String, String),
    /// Disk access errors
    IO(std::io::Error),
    Hyper(hyper::error::Error),
    RecvError(std::sync::mpsc::RecvError),
}

impl Unpin for CaptivePortalError {}

impl std::convert::From<std::convert::Infallible> for CaptivePortalError {
    fn from(error: std::convert::Infallible) -> Self {
        CaptivePortalError::OwnedString(error.to_string())
    }
}

impl std::convert::From<hyper::error::Error> for CaptivePortalError {
    fn from(error: hyper::error::Error) -> Self {
        CaptivePortalError::Hyper(error)
    }
}

impl std::convert::From<std::string::FromUtf8Error> for CaptivePortalError {
    fn from(error: std::string::FromUtf8Error) -> Self {
        CaptivePortalError::Utf8(error.utf8_error())
    }
}

impl std::convert::From<std::string::String> for CaptivePortalError {
    fn from(error: std::string::String) -> Self {
        CaptivePortalError::OwnedString(error)
    }
}

impl std::convert::From<std::sync::mpsc::RecvError> for CaptivePortalError {
    fn from(error: std::sync::mpsc::RecvError) -> Self {
        {
            CaptivePortalError::RecvError(error)
        }
    }
}

impl std::convert::From<std::io::Error> for CaptivePortalError {
    fn from(error: std::io::Error) -> Self {
        CaptivePortalError::IO(error)
    }
}

impl std::convert::From<serde_json::Error> for CaptivePortalError {
    fn from(error: serde_json::Error) -> Self {
        CaptivePortalError::Ser(error)
    }
}

impl std::convert::From<ascii::AsAsciiStrError> for CaptivePortalError {
    fn from(error: ascii::AsAsciiStrError) -> Self {
        CaptivePortalError::Ascii(error)
    }
}

impl std::convert::From<std::str::Utf8Error> for CaptivePortalError {
    fn from(error: std::str::Utf8Error) -> Self {
        CaptivePortalError::Utf8(error)
    }
}

impl std::convert::From<dbus::Error> for CaptivePortalError {
    fn from(error: dbus::Error) -> Self {
        CaptivePortalError::DBus(
            error.name().unwrap_or_default().to_owned(),
            error.message().unwrap_or_default().to_owned(),
        )
    }
}

impl fmt::Display for CaptivePortalError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CaptivePortalError::Generic(m) => write!(f, "{}", m),
            CaptivePortalError::OwnedString(ref m) => write!(f, "{}", m),
            CaptivePortalError::IO(ref e) => e.fmt(f),
            CaptivePortalError::Hyper(ref e) => e.fmt(f),
            CaptivePortalError::Ascii(ref e) => e.fmt(f),
            CaptivePortalError::Utf8(ref e) => e.fmt(f),
            CaptivePortalError::DBus(ref name, ref msg) => {
                write!(f, "Dbus Error: {} - {}", name, msg)
            },
            CaptivePortalError::Ser(ref e) => e.fmt(f),
            CaptivePortalError::RecvError(ref e) => e.fmt(f),
        }
    }
}

impl error::Error for CaptivePortalError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            CaptivePortalError::Generic(ref _m) => None,
            CaptivePortalError::OwnedString(ref _m) => None,
            CaptivePortalError::IO(ref e) => Some(e),
            CaptivePortalError::Hyper(ref e) => Some(e),
            CaptivePortalError::Ascii(ref e) => Some(e),
            CaptivePortalError::Utf8(ref e) => Some(e),
            CaptivePortalError::DBus(_, _) => None,
            CaptivePortalError::Ser(ref e) => Some(e),
            CaptivePortalError::RecvError(ref e) => Some(e),
        }
    }
}

impl CaptivePortalError {
    pub fn network_manager(info: String) -> Self {
        CaptivePortalError::from(format!("network_manager failure: {}", info))
    }
    pub fn ssid(info: String) -> Self {
        CaptivePortalError::from(format!("Invalid ssid: {}", info))
    }
    pub fn pre_shared_key(info: String) -> Self {
        CaptivePortalError::from(format!("Invalid Pre-Shared-Key: {}", info))
    }
    pub fn no_shared_key() -> Self {
        CaptivePortalError::Generic("Passphrase required!")
    }
    pub fn dbus_api(info: String) -> Self {
        CaptivePortalError::from(format!("D-Bus failure: {}", info))
    }
    pub fn start_active_network_manager() -> Self {
        CaptivePortalError::Generic("Failed to start the network manager!")
    }
    pub fn hotspot_failed() -> Self {
        CaptivePortalError::Generic("Failed to initiate a hotspot")
    }
    pub fn no_wifi_device() -> Self {
        CaptivePortalError::Generic("no_wi_fi_device")
    }
    pub fn not_a_wifi_device(info: String) -> Self {
        CaptivePortalError::from(format!("not_awi_fi_device: {}", info))
    }
}
