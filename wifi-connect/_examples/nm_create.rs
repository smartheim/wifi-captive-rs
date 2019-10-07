pub mod shared;

use structopt::StructOpt;

use wifi_captive::network_manager::{errors::{NetworkManagerError, Result}, AccessPointCredentials, NetworkManager, Device};

fn main() -> Result<()> {
    let config: shared::Config = shared::Config::from_args();

    let manager = NetworkManager::new();
    let devices: Vec<Device> = manager.get_wifi_devices()?;
    if devices.is_empty() {
        return Err(NetworkManagerError::Generic("Cannot find a Wifi device"));
    }
    let wifi_device = devices.first().unwrap().as_wifi_device().unwrap();
    let access_points = wifi_device.get_access_points()?;
    let ap = access_points
        .into_iter()
        .find(|ap| ap.ssid.as_ref() == config.ssid.as_bytes())
        .ok_or(NetworkManagerError::OwnedString(format!("Access point {} not found", &config.ssid)))?;

    let credentials = if let Some(pwd) = config.passphrase {
        AccessPointCredentials::Wpa { passphrase: pwd }
    } else {
        AccessPointCredentials::None
    };

    wifi_device.connect(&ap, &credentials)?;

    Ok(())
}
