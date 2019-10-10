use crate::network_manager::connection::delete_connection_if_exists;
use crate::network_manager::device::wifi::WiFiDevice;
use crate::network_manager::{get_service_state, start_service, AccessPoint, AccessPointCredentials, Connection, ConnectionState, Connectivity, Device, NetworkManager, NetworkManagerError, ServiceState, Ssid};
use log::{debug, info};
use std::thread;
use std::time::Duration;
use wifi_captive::Config;
use std::convert::TryInto;

//let access_points = get_access_points(&device)?;
//self.portal_connection = Some(create_portal(&device, &config.ssid, &config.gateway, &config.passphrase)?);

/// Invariant: Portal stopped
fn connect(manager: &NetworkManager, config: &Config) -> Result<bool, NetworkManagerError> {
    delete_connection_if_exists(&manager, &config.ssid);

    let mut devices: Vec<Device> = manager.get_wifi_devices()?;
    if let Some(interface) = &config.interface {
        devices.retain(|f| f.interface() == interface)
    }
    if devices.is_empty() {
        return Err(NetworkManagerError::Generic("Cannot find nm_dbus_generated Wifi device"));
    }
    let wifi_device = devices.first().unwrap().as_wifi_device().unwrap();

    let ssid_label = &config.ssid.to_string();
    let ssid: Ssid = Ssid::from_bytes(ssid_label.as_bytes());

    if let Some(access_point) = wifi_device.find_access_point(&ssid) {
        info!("Connecting to access point '{}'...", ssid_label);

        let credentials = credentials_from_config(config);
        let connect_result = wifi_device.connect(&access_point, &credentials);
        if connect_result.is_err() {
            let err = connect_result.err().unwrap();
            warn!(
                "Error connecting to access point '{}': {:?}",
                ssid_label,
                &err
            );
            return Err(err);
        }
        let (connection, state) = connect_result.unwrap();

        if state == ConnectionState::Activated {
            match wait_for_connectivity(manager, 20) {
                Ok(has_connectivity) => {
                    if has_connectivity {
                        info!("Internet connectivity established");
                    } else {
                        warn!("Cannot establish Internet connectivity");
                    }
                }
                Err(err) => error!("Getting Internet connectivity failed: {}", err),
            }

            return Ok(true);
        }

        if let Err(err) = connection.delete() {
            error!("Deleting connection object failed: {}", err)
        }

        warn!(
            "Connection to access point not activated '{}': {:?}",
            ssid_label, state
        )
    }

    Ok(false)
}

fn wait_for_connectivity(
    manager: &NetworkManager,
    timeout: u64,
) -> Result<bool, NetworkManagerError> {
    let mut total_time = 0;

    loop {
        let connectivity = manager.check_connectivity()?;

        if connectivity == Connectivity::Full || connectivity == Connectivity::Limited {
            debug!(
                "Connectivity established: {:?} / {}s elapsed",
                connectivity, total_time
            );

            return Ok(true);
        } else if total_time >= timeout {
            debug!(
                "Timeout reached in waiting for connectivity: {:?} / {}s elapsed",
                connectivity, total_time
            );

            return Ok(false);
        }

        ::std::thread::sleep(::std::time::Duration::from_secs(1));

        total_time += 1;

        debug!(
            "Still waiting for connectivity: {:?} / {}s elapsed",
            connectivity, total_time
        );
    }
}
