mod shared;

use structopt::StructOpt;

use wifi_captive::network_manager::device::get_devices;
use wifi_captive::network_manager::errors::{NetworkManagerError, Result};
use wifi_captive::network_manager::Device;
use wifi_captive::network_manager::DeviceType;
use wifi_captive::network_manager::NetworkManager;

use std::convert::TryInto;

fn main() -> Result<()> {
    let config: shared::Config = shared::Config::from_args();
    let manager = NetworkManager::new();

    let mut devices: Vec<Device> = manager.get_wifi_devices()?;
    if let Some(interface) = &config.interface {
        devices.retain(|f| f.interface() == interface)
    }
    if devices.is_empty() {
        return Err(NetworkManagerError::Generic("Cannot find nm_dbus_generated Wifi device"));
    }
    let wifi_device = devices.first().unwrap().as_wifi_device().unwrap();
    wifi_device.create_hotspot(config.ssid.try_into()?,config.passphrase,None)?;

    Ok(())
}
