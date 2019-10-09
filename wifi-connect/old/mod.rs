pub mod ssid;
mod state;
mod wifi_settings;

pub use ssid::*;
pub use state::*;
pub use wifi_settings::*;

use std::fmt;

use super::{dbus::path_to_string, *};
use ::dbus::arg::{Array, RefArg};
use ::dbus::Path;
use std::collections::HashMap;

#[derive(Clone)]
pub struct Connection {
    dbus_manager: NetworkManager,
    path: String,
    pub settings: WiFiConnectionSettings,
}

impl Connection {
    pub(crate) fn init(
        dbus_manager: NetworkManager,
        path: String,
    ) -> Result<Self, NetworkManagerError> {
        let settings = get_connection_settings(&dbus_manager, &path)?;

        Ok(Connection {
            dbus_manager,
            path,
            settings,
        })
    }

    pub fn get_state(&self) -> Result<ConnectionState, NetworkManagerError> {
        //TODO
        let active_path_option = get_active_connection_by_path(&self.dbus_manager, &self.path)?;

        if let Some(active_path) = active_path_option {
            let state = get_connection_state(self.dbus_manager.clone(), &active_path)?;

            Ok(state)
        } else {
            Ok(ConnectionState::Deactivated)
        }
    }

    pub fn delete(&self) -> Result<(), NetworkManagerError> {
        delete_connection(&self.dbus_manager, &self.path)
    }

    /// Activate nm_dbus_generated Network Manager connection.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use network_manager::NetworkManager;
    /// let manager = NetworkManager::new();
    /// let connections = manager.get_connections().unwrap();
    /// connections[0].activate().unwrap();
    /// ```
    pub fn activate(&self) -> Result<ConnectionState, NetworkManagerError> {
        let state = self.get_state()?;

        match state {
            ConnectionState::Activated => Ok(ConnectionState::Activated),
            ConnectionState::Activating => wait_for_connection(
                self,
                &ConnectionState::Activated,
                self.dbus_manager.method_timeout(),
            ),
            ConnectionState::Unknown => {
                return Err(NetworkManagerError::network_manager(
                    "Unable to get connection state".into(),
                ))
            },
            _ => {
                activate_connection(&self.dbus_manager, &self.path)?;

                wait_for_connection(
                    self,
                    &ConnectionState::Activated,
                    self.dbus_manager.method_timeout(),
                )
            },
        }
    }

    /// Deactivates nm_dbus_generated Network Manager connection.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use network_manager::NetworkManager;
    /// let manager = NetworkManager::new();
    /// let connections = manager.get_connections().unwrap();
    /// connections[0].deactivate().unwrap();
    /// ```
    pub fn deactivate(&self) -> Result<ConnectionState, NetworkManagerError> {
        let state = self.get_state()?;

        match state {
            ConnectionState::Deactivated => Ok(ConnectionState::Deactivated),
            ConnectionState::Deactivating => wait_for_connection(
                self,
                &ConnectionState::Deactivated,
                self.dbus_manager.method_timeout(),
            ),
            ConnectionState::Unknown => Err(NetworkManagerError::network_manager(
                "Unable to get connection state".into(),
            )),
            _ => {
                let active_path_option =
                    get_active_connection_by_path(&self.dbus_manager, &self.path)?;

                if let Some(active_path) = active_path_option {
                    deactivate_connection(&self.dbus_manager, &active_path)?;

                    wait_for_connection(
                        self,
                        &ConnectionState::Deactivated,
                        self.dbus_manager.method_timeout(),
                    )
                } else {
                    Ok(ConnectionState::Deactivated)
                }
            },
        }
    }

    pub fn get_devices(&self) -> Result<Vec<Device>, NetworkManagerError> {
        let active_path_option = get_active_connection_by_path(&self.dbus_manager, &self.path)?;
        if let Some(active_path) = active_path_option {
            Ok(get_active_connection_devices(
                &self.dbus_manager,
                &active_path,
            )?)
        } else {
            Ok(vec![])
        }
    }
}

impl Ord for Connection {
    fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {
        i32::from(self).cmp(&i32::from(other))
    }
}

impl PartialOrd for Connection {
    fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Connection {
    fn eq(&self, other: &Connection) -> bool {
        i32::from(self) == i32::from(other)
    }
}

impl Eq for Connection {}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Connection {{ path: {:?}, settings: {:?} }}",
            self.path, self.settings
        )
    }
}

impl<'a> From<&'a Connection> for i32 {
    fn from(val: &Connection) -> i32 {
        val.clone()
            .path
            .rsplit('/')
            .nth(0)
            .unwrap()
            .parse::<i32>()
            .unwrap()
    }
}

pub fn list_connection_paths(
    dbus_manager: &NetworkManager,
) -> Result<Vec<String>, NetworkManagerError> {
    let response =
        dbus_manager
            .dbus
            .call(NM_SETTINGS_PATH, NM_SETTINGS_INTERFACE, "ListConnections")?;

    let array: Array<Path, _> = dbus_manager.dbus.extract(&response)?;

    Ok(array.map(|e| e.to_string()).collect())
}

pub fn list_active_connection_paths(
    dbus_manager: &NetworkManager,
) -> Result<Vec<String>, NetworkManagerError> {
    dbus_manager
        .dbus
        .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "ActiveConnections")
}

/// Get nm_dbus_generated list of Network Manager connections sorted by path.
///
/// # Examples
///
/// ```
/// use network_manager::NetworkManager;
/// let manager = NetworkManager::new();
/// let connections = list_connections(&manager).unwrap();
/// println!("{:?}", connections);
/// ```
pub fn list_connections(
    dbus_manager: &NetworkManager,
) -> Result<Vec<Connection>, NetworkManagerError> {
    let device_paths = list_connection_paths(dbus_manager)?;
    let mut result = Vec::with_capacity(device_paths.len());

    for path in device_paths {
        let device = Connection::init(dbus_manager.clone(), path)?;
        result.push(device);
    }
    result.sort();
    Ok(result)
}

pub fn list_active_connections(
    dbus_manager: &NetworkManager,
) -> Result<Vec<Connection>, NetworkManagerError> {
    let device_paths = list_active_connection_paths(dbus_manager)?;
    let mut result = Vec::with_capacity(device_paths.len());
    for path in device_paths {
        if let Some(path) = get_active_connection_path(dbus_manager, &path) {
            result.push(Connection::init(dbus_manager.clone(), path.clone())?)
        }
    }
    result.sort();
    Ok(result)
}

fn get_active_connection_path(dbus_manager: &NetworkManager, path: &str) -> Option<String> {
    dbus_manager
        .dbus
        .property(path, NM_ACTIVE_INTERFACE, "Connection")
        .ok()
}

pub fn get_active_connection_by_path(
    dbus_manager: &NetworkManager,
    connection_path: &str,
) -> Result<Option<String>, NetworkManagerError> {
    let active_paths = list_active_connection_paths(dbus_manager)?;

    for active_path in active_paths {
        if let Some(settings_path) = get_active_connection_path(dbus_manager, &active_path) {
            if connection_path == settings_path {
                return Ok(Some(active_path));
            }
        }
    }

    Ok(None)
}

pub fn get_active_connection_device_paths(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<Vec<String>, NetworkManagerError> {
    dbus_manager
        .dbus
        .property(path, NM_ACTIVE_INTERFACE, "Devices")
}

pub fn get_active_connection_devices(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<Vec<Device>, NetworkManagerError> {
    let device_paths = get_active_connection_device_paths(dbus_manager, path)?;
    let mut result = Vec::with_capacity(device_paths.len());

    for path in device_paths {
        let device = Device::try_new(dbus_manager.clone(), path)?;
        result.push(device);
    }

    Ok(result)
}

pub fn delete_connection(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<(), NetworkManagerError> {
    dbus_manager
        .dbus
        .call(path, NM_CONNECTION_INTERFACE, "Delete")
        .map(|_| ())
}

pub fn activate_connection(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<(), NetworkManagerError> {
    dbus_manager
        .dbus
        .call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "ActivateConnection",
            &[
                &Path::new(path)? as &dyn RefArg,
                &Path::new("/")? as &dyn RefArg,
                &Path::new("/")? as &dyn RefArg,
            ],
        )
        .map(|_| ())
}

pub fn deactivate_connection(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<(), NetworkManagerError> {
    dbus_manager
        .dbus
        .call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "DeactivateConnection",
            &[&Path::new(path)? as &dyn RefArg],
        )
        .map(|_| ())
}

/// Adds necessary entries to the given settings map.
/// To be used by wifi device connect and [`add_wifi_connection`].
pub fn prepare_wifi_security_settings(
    credentials: &AccessPointCredentials,
    settings: &mut HashMap<String, VariantMap>,
) -> Result<(), NetworkManagerError> {
    match *credentials {
        AccessPointCredentials::Wep { ref passphrase } => {
            let mut security_settings: VariantMap = HashMap::new();

            add_val(
                &mut security_settings,
                "wep-key-type",
                NM_WEP_KEY_TYPE_PASSPHRASE,
            );
            add_string(
                &mut security_settings,
                "wep-key0",
                verify_ascii_password(passphrase.clone())?,
            );

            settings.insert("802-11-wireless-security".to_string(), security_settings);
        },
        AccessPointCredentials::Wpa { ref passphrase } => {
            let mut security_settings: VariantMap = HashMap::new();

            add_str(&mut security_settings, "key-mgmt", "wpa-psk");
            add_string(
                &mut security_settings,
                "psk",
                verify_ascii_password(passphrase.clone())?,
            );

            settings.insert("802-11-wireless-security".to_string(), security_settings);
        },
        AccessPointCredentials::Enterprise {
            ref identity,
            ref passphrase,
        } => {
            let mut security_settings: VariantMap = HashMap::new();

            add_str(&mut security_settings, "key-mgmt", "wpa-eap");

            let mut eap: VariantMap = HashMap::new();
            add_val(&mut eap, "eap", vec!["peap".to_string()]);
            add_str(&mut eap, "identity", identity as &str);
            add_str(&mut eap, "password", passphrase as &str);
            add_str(&mut eap, "phase2-auth", "mschapv2");

            settings.insert("802-11-wireless-security".to_string(), security_settings);
            settings.insert("802-1x".to_string(), eap);
        },
        AccessPointCredentials::None => {},
    };
    Ok(())
}

pub fn add_wifi_connection(
    dbus_manager: &NetworkManager,
    ssid: &str,
    interface: &str,
    credentials: &AccessPointCredentials,
) -> Result<Connection, NetworkManagerError> {
    let mut settings: HashMap<String, VariantMap> = HashMap::new();

    let mut wireless: VariantMap = HashMap::new();
    add_val(&mut wireless, "ssid", ssid.as_bytes().to_vec());
    settings.insert("802-11-wireless".to_string(), wireless);

    prepare_wifi_security_settings(credentials, &mut settings)?;

    let mut connection: VariantMap = HashMap::new();
    add_str(&mut connection, "type", "802-11-wireless");
    add_str(&mut connection, "interface-name", interface);
    add_str(&mut connection, "id", ssid);
    settings.insert("connection".to_string(), connection);

    let response = dbus_manager.dbus.call_with_args(
        NM_SETTINGS_PATH,
        NM_SETTINGS_INTERFACE,
        "AddConnection",
        &[&settings as &dyn RefArg],
    )?;

    let path: Path = dbus_manager.dbus.extract(&response)?;
    let path = path_to_string(&path)?;

    let connection = Connection::init(dbus_manager.clone(), path)?;

    Ok(connection)
}

pub fn delete_access_point_connections(
    manager: &NetworkManager,
) -> Result<(), NetworkManagerError> {
    let connections = list_connections(&manager)?;

    for connection in connections {
        if &connection.settings.kind == "802-11-wireless" && &connection.settings.mode == "ap" {
            debug!(
                "Deleting access point connection profile: {:?}",
                connection.settings.ssid,
            );
            connection.delete()?;
        }
    }

    Ok(())
}

pub fn find_connection(manager: &NetworkManager, ssid: &str) -> Option<Connection> {
    let connections = match list_connections(&manager) {
        Ok(connections) => connections,
        Err(e) => {
            error!("Getting existing connections failed: {}", e);
            return None;
        },
    };

    for connection in connections {
        if let Ok(connection_ssid) = connection.settings.ssid.to_string() {
            if &connection.settings.kind == "802-11-wireless" && connection_ssid == ssid {
                return Some(connection);
            }
        }
    }
    None
}

pub fn delete_connection_if_exists(manager: &NetworkManager, ssid: &str) {
    let connections = match list_connections(&manager) {
        Ok(connections) => connections,
        Err(e) => {
            error!("Getting existing connections failed: {}", e);
            return;
        },
    };

    for connection in connections {
        if let Ok(connection_ssid) = connection.settings.ssid.to_string() {
            if &connection.settings.kind == "802-11-wireless" && connection_ssid == ssid {
                info!(
                    "Deleting existing WiFi connection: {:?}",
                    connection.settings.ssid,
                );

                if let Err(e) = connection.delete() {
                    error!("Deleting existing WiFi connection failed: {}", e);
                }
            }
        }
    }
}

pub fn wait_for_connection(
    connection: &Connection,
    target_state: &ConnectionState,
    timeout: u64,
) -> Result<ConnectionState, NetworkManagerError> {
    if timeout == 0 {
        return connection.get_state();
    }

    debug!("Waiting for connection state: {:?}", target_state);

    let mut total_time = 0;

    loop {
        ::std::thread::sleep(::std::time::Duration::from_secs(1));

        let state = connection.get_state()?;

        total_time += 1;

        if state == *target_state {
            debug!(
                "Connection target state reached: {:?} / {}s elapsed",
                state, total_time
            );

            return Ok(state);
        } else if total_time >= timeout {
            debug!(
                "Timeout reached in waiting for connection state ({:?}): {:?} / {}s elapsed",
                target_state, state, total_time
            );

            return Ok(state);
        }

        debug!(
            "Still waiting for connection state ({:?}): {:?} / {}s elapsed",
            target_state, state, total_time
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::NetworkManager;
    use super::*;

    #[test]
    fn test_connection_enable_disable() {
        let manager = NetworkManager::new();

        let connections = list_connections(&manager).unwrap();

        // set environment variable $TEST_WIFI_SSID with the wifi's ssid that you want to test
        // e.g.  export TEST_WIFI_SSID="Resin.io Wifi"
        let wifi_env_var = "TEST_WIFI_SSID";
        let connection = match ::std::env::var(wifi_env_var) {
            Ok(ssid) => connections
                .iter()
                .filter(|c| c.settings.ssid.to_string().unwrap() == ssid)
                .nth(0)
                .unwrap()
                .clone(),
            Err(e) => panic!(
                "couldn't retrieve environment variable {}: {}",
                wifi_env_var, e
            ),
        };

        let state = connection.get_state().unwrap();

        if state == ConnectionState::Activated {
            let state = connection.deactivate().unwrap();
            assert_eq!(ConnectionState::Deactivated, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));

            let state = connection.activate().unwrap();
            assert_eq!(ConnectionState::Activated, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));
        } else {
            let state = connection.activate().unwrap();
            assert_eq!(ConnectionState::Activated, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));

            let state = connection.deactivate().unwrap();
            assert_eq!(ConnectionState::Deactivated, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));
        }
    }
}
