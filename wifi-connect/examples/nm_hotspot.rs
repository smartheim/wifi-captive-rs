use clap::{App, Arg};

use wifi_captive::network_manager::{errors::{Result, NetworkManagerError}, Device, DeviceType, NetworkManager};

fn main() -> Result<()> {
    let matches = App::new(file!())
        .arg(
            Arg::with_name("INTERFACE")
                .short("i")
                .takes_value(true)
                .required(false)
                .help("Network interface"),
        )
        .arg(
            Arg::with_name("ssid")
                .takes_value(true)
                .required(true)
                .help("Network ssid"),
        )
        .arg(
            Arg::with_name("PASSWORD")
                .takes_value(true)
                .required(false)
                .help("Network password"),
        )
        .get_matches();

    let manager = NetworkManager::new();

    let device = find_device(&manager, matches.value_of("INTERFACE"))?;
    let wifi_device = device.as_wifi_device().unwrap();

    wifi_device.create_hotspot(
        matches.value_of("ssid").unwrap(),
        matches.value_of("PASSWORD"),
        None,
    )?;

    Ok(())
}

fn find_device(manager: &NetworkManager, interface: Option<&str>) -> Result<Device> {
    if let Some(interface) = interface {
        let device = manager.get_device_by_interface(interface)?;

        if *device.device_type() == DeviceType::WiFi {
            Ok(device)
        } else {
            Err(NetworkManagerError::from(format!(
                "{} is not a WiFi device",
                interface
            )))
        }
    } else {
        let devices = manager.get_devices()?;

        let index = devices
            .iter()
            .position(|d| *d.device_type() == DeviceType::WiFi);

        if let Some(index) = index {
            Ok(devices[index].clone())
        } else {
            Err(NetworkManagerError::Generic("Cannot find a WiFi device"))
        }
    }
}
