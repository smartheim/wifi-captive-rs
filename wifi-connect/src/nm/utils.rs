use super::{device, networkmanager, NM_INTERFACE, NM_PATH};
use crate::CaptivePortalError;
use dbus::nonblock;
use std::sync::Arc;

/// Finds the first wifi device or the wifi device on the given device interface.
/// Returns (wifi_device_path, interface_name) on success and an error otherwise.
pub async fn find_wifi_device(
    connection: Arc<dbus::nonblock::SyncConnection>,
    preferred_interface: &Option<String>,
) -> Result<(String, String), CaptivePortalError> {
    let p = nonblock::Proxy::new(NM_INTERFACE, NM_PATH, connection.clone());

    // Get all devices (if possible: by interface)
    use networkmanager::NetworkManager;
    if let Some(interface_name) = preferred_interface {
        let device_path = p.get_device_by_ip_iface(&interface_name).await?;
        let device_data = nonblock::Proxy::new(NM_INTERFACE, &device_path, connection.clone());
        use device::Device;
        let dtype = device_data.device_type().await?;
        if dtype == super::connectivity::DeviceType::WiFi as u32 {
            info!("Wireless device found: {}", interface_name);
            return Ok((device_path.to_string(), interface_name.clone()));
        }
    };

    // Filter by type; only wifi devices; take first
    let device_paths = p.get_all_devices().await?;
    for device_path in device_paths {
        let device_data = nonblock::Proxy::new(NM_INTERFACE, &device_path, connection.clone());
        use device::Device;
        let dtype = device_data.device_type().await?;
        if dtype == super::connectivity::DeviceType::WiFi as u32 {
            let interface = device_data.interface().await?;
            info!("Wireless device on '{}'", &interface);
            return Ok((device_path.to_string(), interface));
        }
    }

    Err(CaptivePortalError::no_wifi_device())
}
