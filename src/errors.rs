//! # Error and Result Type
//!
//! This crate uses one wrapping error type.
//! Sub-modules and methods should return a specific error type whenever possible though.

use crate::NetworkManagerState;
use hyper::http;
use std::error;
use std::fmt;

/// The main error type used throughout this crate. It wraps / converts from nm_dbus_generated few other error
/// types and implements [error::Error] so that you can use it in any situation where the
/// standard error type is expected.
#[derive(Debug)]
pub enum CaptivePortalError {
    /// Generic errors are very rarely used and only used if no other error type matches
    Generic(String),
    /// Serialisation failed
    Ser(serde_json::Error),
    Utf8(std::str::Utf8Error),
    // Name, Message
    DBus(String, String),
    /// IO Error with context
    IO(std::io::Error, &'static str),
    Hyper(hyper::error::Error),
    RecvError(std::sync::mpsc::RecvError),
    IwdError(&'static str),

    DhcpError(&'static str),
    HttpRoutingFailed,
    NotInStationMode,
    NotRequiredConnectivity(NetworkManagerState),
    HotspotFailed,
    NoWifiDeviceFound,
    InvalidSharedKey(String),
    NoSharedKeyProvided,
}

impl Unpin for CaptivePortalError {}

impl std::convert::From<std::convert::Infallible> for CaptivePortalError {
    fn from(error: std::convert::Infallible) -> Self {
        CaptivePortalError::Generic(error.to_string())
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
        CaptivePortalError::Generic(error.to_string())
    }
}

impl std::convert::From<std::string::String> for CaptivePortalError {
    fn from(error: std::string::String) -> Self {
        CaptivePortalError::Generic(error)
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
        CaptivePortalError::IO(error, "")
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
            CaptivePortalError::Generic(ref m) => m.fmt(f),
            CaptivePortalError::IO(ref e, str) => write!(f, "{} - {}", str, e),
            CaptivePortalError::Hyper(ref e) => e.fmt(f),
            CaptivePortalError::Utf8(ref e) => e.fmt(f),
            CaptivePortalError::DBus(ref name, ref msg) => write!(f, "Dbus Error: {} - {}", name, msg),
            CaptivePortalError::Ser(ref e) => e.fmt(f),
            CaptivePortalError::RecvError(ref e) => e.fmt(f),
            CaptivePortalError::NotInStationMode => write!(f, "Scanning not possible: Not in station mode!"),
            CaptivePortalError::NotRequiredConnectivity(_) => write!(f, "Connectivity is limited"),
            CaptivePortalError::HotspotFailed => write!(f, "Failed to initiate a hotspot"),
            CaptivePortalError::NoWifiDeviceFound => write!(f, "No wifi device found on this system"),
            CaptivePortalError::InvalidSharedKey(ref m) => write!(f, "Invalid Passphrase: {}", m),
            CaptivePortalError::NoSharedKeyProvided => write!(f, "Passphrase required!"),
            CaptivePortalError::HttpRoutingFailed => write!(f, "Failed to internally route http data"),
            CaptivePortalError::DhcpError(str) => str.fmt(f),
            CaptivePortalError::IwdError(str) => str.fmt(f),
        }
    }
}

impl error::Error for CaptivePortalError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            CaptivePortalError::IO(ref e, _str) => Some(e),
            CaptivePortalError::Hyper(ref e) => Some(e),
            CaptivePortalError::Utf8(ref e) => Some(e),
            CaptivePortalError::Ser(ref e) => Some(e),
            CaptivePortalError::RecvError(ref e) => Some(e),
            _ => None,
        }
    }
}
