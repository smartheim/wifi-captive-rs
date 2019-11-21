//! This crate will immediately quit if no wifi device can be found. This module encapsulates the
//! method to find a wifi device via the network manager dbus API.

use super::NM_BUSNAME;
use crate::CaptivePortalError;
use dbus::nonblock;
use std::sync::Arc;

pub(crate) struct FindWifiDeviceResult {
    /// The network manager dbus api device path
    pub device_path: dbus::Path<'static>,
    /// The interface name
    pub interface_name: String,
    /// The mac address
    pub hw: String,
}

/// Finds the first wifi device or the wifi device on the given device interface.
/// Returns (wifi_device_path, interface_name) on success and an error otherwise.
pub(crate) async fn find_wifi_device(
    connection: Arc<dbus::nonblock::SyncConnection>,
    preferred_interface: &Option<String>,
) -> Result<FindWifiDeviceResult, CaptivePortalError> {
    let p = nonblock::Proxy::new(NM_BUSNAME, "/", connection.clone());
    use super::generated::iwd::OrgFreedesktopDBusObjectManager;

    // Get all devices (if possible: by interface)
    let objects = p.get_managed_objects().await?;
    for (device_path, entry) in objects {
        if let Some(entry) = entry.get("net.connman.iwd.Device") {
            let device_hw = entry
                .get("Address")
                .ok_or(CaptivePortalError::Generic(
                    "net.connman.iwd.Device: Must have an 'Address'",
                ))?
                .0
                .as_str()
                .ok_or(CaptivePortalError::Generic(
                    "net.connman.iwd.Device/Address: Expects a string!",
                ))?;
            let device_interface = entry
                .get("Name")
                .ok_or(CaptivePortalError::Generic(
                    "net.connman.iwd.Device: Must have a 'Name'",
                ))?
                .0
                .as_str()
                .ok_or(CaptivePortalError::Generic(
                    "net.connman.iwd.Device/Name: Expects a string!",
                ))?;

            if let Some(interface_name) = preferred_interface {
                if &interface_name[..] != device_interface {
                    info!(
                        "Wireless device found: {}. Skipping because user requested: {}",
                        device_interface, &interface_name
                    );
                    continue;
                }
            }
            info!("Wireless device found: {}", device_interface);
            return Ok(FindWifiDeviceResult {
                device_path,
                interface_name: device_interface.to_owned(),
                hw: device_hw.to_owned(),
            });
        }
    }

    Err(CaptivePortalError::NoWifiDeviceFound)
}
