//! # A network backend implementation. Either network manager or iwd.
//! This depends on the cargo feature flag. Either "networkmanager" or "iwd".

#[cfg(feature = "iwd")]
mod iwd;

#[cfg(feature = "networkmanager")]
mod nm;

#[cfg(feature = "iwd")]
pub use iwd::*;
#[cfg(feature = "networkmanager")]
pub use nm::*;
