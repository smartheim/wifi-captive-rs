//! # The Network Manager Library
//!
//! The Network Manager Library provides the essential
//! functionality for configuring Network Manager from Rust.

pub mod errors;

pub mod connection;
mod dbus;
pub mod device;
mod nm;
mod nm_state;
mod utils;

pub type VariantMap = HashMap<String, Variant<Box<dyn RefArg>>>;

const NM_SERVICE_MANAGER: &str = "org.freedesktop.NetworkManager";

pub const NM_SERVICE_PATH: &str = "/org/freedesktop/NetworkManager";
pub const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";

pub const NM_SERVICE_INTERFACE: &str = "org.freedesktop.NetworkManager";
pub const NM_SETTINGS_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings";
pub const NM_CONNECTION_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings.\
                                           Connection";
pub const NM_ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const NM_DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";
pub const NM_WIRELESS_INTERFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub const NM_ACCESS_POINT_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";

pub const NM_WEP_KEY_TYPE_PASSPHRASE: u32 = 2;

pub const UNKNOWN_CONNECTION: &str = "org.freedesktop.NetworkManager.UnknownConnection";
pub const METHOD_RETRY_ERROR_NAMES: &[&str; 1] = &[UNKNOWN_CONNECTION];

pub use self::dbus::{get_service_state, start_service, stop_service, ServiceState};
pub use connection::{Connection, ConnectionState, Ssid, WiFiConnectionSettings};
pub use device::state::{DeviceState, DeviceType};
pub use device::wifi_ap;
pub use device::wifi_ap::{AccessPoint, AccessPointCredentials};
pub use device::wifi_sta;
pub use device::wifi_sta::create_hotspot;
pub use device::Device;
pub use errors::NetworkManagerError;
pub use nm::NetworkManager;
pub use nm_state::{Connectivity, NetworkManagerState};

pub(crate) use utils::*;

use ::dbus::arg::{RefArg, Variant};
use std::collections::HashMap;
