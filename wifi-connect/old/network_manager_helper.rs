use crate::network_manager::connection::delete_connection_if_exists;
use crate::network_manager::device::wifi::WiFiDevice;
use crate::network_manager::{get_service_state, start_service, AccessPoint, AccessPointCredentials, Connection, ConnectionState, Connectivity, Device, NetworkManager, NetworkManagerError, ServiceState, Ssid};
use log::{debug, info};
use std::thread;
use std::time::Duration;
use wifi_captive::Config;
use std::convert::TryInto;

pub fn start_network_manager_service() -> Result<(), NetworkManagerError> {
    let state = match get_service_state() {
        Ok(state) => state,
        _ => {
            info!("Cannot get the network_manager service state");
            return Ok(());
        }
    };

    if state != ServiceState::Active {
        let state = start_service(15)?;
        if state != ServiceState::Active {
            return Err(NetworkManagerError::start_active_network_manager());
        } else {
            info!("network_manager service started successfully");
        }
    } else {
        debug!("network_manager service already running");
    }

    Ok(())
}

fn get_access_points_ssids(access_points: &[AccessPoint]) -> Vec<String> {
    access_points
        .iter()
        .filter_map(|ap| ap.ssid.to_string().ok())
        .collect()
}

//fn create_portal(
//    device: &Device,
//    ssid: &str,
//    gateway: &Ipv4Addr,
//    passphrase: &Option<String>,
//) -> Result<Connection, NetworkManagerError> {
//    info!("Starting access point...");
//    let wifi_device = device.as_wifi_device().unwrap();
//    let (portal_connection, _) = wifi_device.create_hotspot(ssid, passphrase, Some(*gateway))?;
//    info!("Access point '{}' created", ssid);
//    Ok(portal_connection)
//}

fn stop_portal(connection: &Connection, ssid: &str) -> Result<(), NetworkManagerError> {
    info!("Stopping access point '{}'...", ssid);
    connection.deactivate()?;
    connection.delete()?;
    thread::sleep(Duration::from_secs(1));
    info!("Access point '{}' stopped", ssid);
    Ok(())
}

fn get_access_points(device: &Device) -> Result<Vec<AccessPoint>, NetworkManagerError> {
    let retries_allowed = 10;
    let mut retries = 0;

    // After stopping the hotspot we may have to wait nm_dbus_generated bit for the list
    // of access points to become available
    while retries < retries_allowed {
        let wifi_device = device.as_wifi_device().unwrap();
        let mut access_points = wifi_device.get_access_points()?;

        access_points.retain(|ap| ap.ssid.to_string().is_ok());

        if !access_points.is_empty() {
            info!(
                "Access points: {:?}",
                get_access_points_ssids(&access_points)
            );
            return Ok(access_points);
        }

        retries += 1;
        debug!("No access points found - retry #{}", retries);
        thread::sleep(Duration::from_secs(1));
    }

    warn!("No access points found - giving up...");
    Ok(vec![])
}

fn credentials_from_config(config: &Config) -> AccessPointCredentials {
    if let Some(identity) = &config.identity {
        AccessPointCredentials::Enterprise {
            identity: identity.clone(),
            passphrase: match config.passphrase {
                Some(ref v) => v.clone(),
                None => String::new(),
            },
        }
    } else if let Some(pwd) = &config.passphrase {
        AccessPointCredentials::Wpa {
            passphrase: pwd.clone(),
        }
    } else {
        AccessPointCredentials::None
    }
}

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
