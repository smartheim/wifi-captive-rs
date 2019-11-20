//! # Error and Result Type
//!
//! This crate uses one wrapping error type.
//! Sub-modules and methods should return a specific error type whenever possible though.

use hyper::http;
use std::error;
use std::fmt;

/// The main error type used throughout this crate. It wraps / converts from nm_dbus_generated few other error
/// types and implements [error::Error] so that you can use it in any situation where the
/// standard error type is expected.
#[derive(Debug)]
pub enum CaptivePortalError {
    /// Generic errors are very rarely used and only used if no other error type matches
    Generic(&'static str),
    GenericO(String),
    /// Serialisation failed
    Ser(serde_json::Error),
    Utf8(std::str::Utf8Error),
    // Name, Message
    DBus(String, String),
    /// Disk access errors
    IO(std::io::Error),
    Hyper(hyper::error::Error),
    RecvError(std::sync::mpsc::RecvError),
    NotInStationMode,
}

impl Unpin for CaptivePortalError {}

impl std::convert::From<std::convert::Infallible> for CaptivePortalError {
    fn from(error: std::convert::Infallible) -> Self {
        CaptivePortalError::GenericO(error.to_string())
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

impl std::convert::From<hyper::header::ToStrError> for CaptivePortalError {
    fn from(error: http::header::ToStrError) -> Self {
        CaptivePortalError::GenericO(error.to_string())
    }
}

impl std::convert::From<std::string::String> for CaptivePortalError {
    fn from(error: std::string::String) -> Self {
        CaptivePortalError::GenericO(error)
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
            CaptivePortalError::GenericO(ref m) => write!(f, "{}", m),
            CaptivePortalError::IO(ref e) => e.fmt(f),
            CaptivePortalError::Hyper(ref e) => e.fmt(f),
            CaptivePortalError::Utf8(ref e) => e.fmt(f),
            CaptivePortalError::DBus(ref name, ref msg) => {
                write!(f, "Dbus Error: {} - {}", name, msg)
            },
            CaptivePortalError::Ser(ref e) => e.fmt(f),
            CaptivePortalError::RecvError(ref e) => e.fmt(f),
            CaptivePortalError::NotInStationMode => {
                write!(f, "Scanning not possible: Not in station mode!")
            },
        }
    }
}

impl error::Error for CaptivePortalError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            CaptivePortalError::IO(ref e) => Some(e),
            CaptivePortalError::Hyper(ref e) => Some(e),
            CaptivePortalError::Utf8(ref e) => Some(e),
            CaptivePortalError::Ser(ref e) => Some(e),
            CaptivePortalError::RecvError(ref e) => Some(e),
            _ => None,
        }
    }
}

impl CaptivePortalError {
    pub fn invalid_shared_key(info: String) -> Self {
        CaptivePortalError::from(format!("Invalid Passphrase: {}", info))
    }
    pub fn no_shared_key() -> Self {
        CaptivePortalError::Generic("Passphrase required!")
    }
    pub fn hotspot_failed() -> Self {
        CaptivePortalError::Generic("Failed to initiate a hotspot")
    }
    pub fn no_wifi_device() -> Self {
        CaptivePortalError::Generic("No wifi device found on this system")
    }
}
