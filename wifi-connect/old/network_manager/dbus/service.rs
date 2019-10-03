use dbus::arg::{Dict, Iter, Variant};
use dbus::{Message, Path};
use dbus::blocking::{Connection, BlockingSender};
use dbus::ffidisp::ConnectionItem;
use dbus::blocking::stdintf::org_freedesktop_dbus::*;


use futures::Future;
use futures_cpupool::CpuPool;
use std::str::FromStr;
use std::time::Duration;
use tokio_timer::Timer;

use super::super::NetworkManagerError;
use dbus::message::MatchRule;

const SD_SERVICE_MANAGER: &str = "org.freedesktop.systemd1";
const SD_SERVICE_PATH: &str = "/org/freedesktop/systemd1";
const SD_MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
const SD_UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";

/// Starts the Network Manager service.
///
/// # Examples
///
/// ```
/// use network_manager::NetworkManager;
/// let state = NetworkManager::start_service(10).unwrap();
/// println!("{:?}", state);
/// ```
pub fn start_service(timeout: u64) -> Result<ServiceState, NetworkManagerError> {
    let state = get_service_state()?;
    match state {
        ServiceState::Active => Ok(state),
        ServiceState::Activating => handler(timeout, ServiceState::Active),
        ServiceState::Failed => Err(NetworkManagerError::Generic("Service")),
        _ => {
            let message = Message::new_method_call(
                SD_SERVICE_MANAGER,
                SD_SERVICE_PATH,
                SD_MANAGER_INTERFACE,
                "StartUnit",
            )
                .map_err(|_| NetworkManagerError::Generic("Service"))?
                .append2("network_manager.service", "fail");

            let connection = Connection::new_system()
                .map_err(|_| NetworkManagerError::Generic("Service"))?;

            connection
                .send_with_reply_and_block(message, Duration::from_millis(2000))
                .map_err(|_| NetworkManagerError::Generic("Service"))?;

            handler(timeout, ServiceState::Active)
        }
    }
}

/// Stops the Network Manager service.
///
/// # Examples
///
/// ```
/// use network_manager::NetworkManager;
/// let state = NetworkManager::stop_service(10).unwrap();
/// println!("{:?}", state);
/// ```
pub fn stop_service(timeout: u64) -> Result<ServiceState, NetworkManagerError> {
    let state = get_service_state()?;
    match state {
        ServiceState::Inactive => Ok(state),
        ServiceState::Deactivating => handler(timeout, ServiceState::Inactive),
        ServiceState::Failed => Err(NetworkManagerError::Generic("")),
        _ => {
            let message = Message::new_method_call(
                SD_SERVICE_MANAGER,
                SD_SERVICE_PATH,
                SD_MANAGER_INTERFACE,
                "StopUnit",
            )
                .map_err(|_| NetworkManagerError::Generic("Service"))?
                .append2("network_manager.service", "fail");

            let connection = Connection::new_system()
                .map_err(|_| NetworkManagerError::Generic("Service"))?;

            connection
                .send_with_reply_and_block(message, Duration::from_millis(2000))
                .map_err(|_| NetworkManagerError::Generic("Service"))?;

            handler(timeout, ServiceState::Inactive)
        }
    }
}

/// Gets the state of the Network Manager service.
///
/// # Examples
///
/// ```
/// use network_manager::NetworkManager;
/// let state = NetworkManager::get_service_state().unwrap();
/// println!("{:?}", state);
/// ```
pub fn get_service_state() -> Result<ServiceState, NetworkManagerError> {
    let message = Message::new_method_call(
        SD_SERVICE_MANAGER,
        SD_SERVICE_PATH,
        SD_MANAGER_INTERFACE,
        "GetUnit",
    )
        .map_err(|_| NetworkManagerError::Generic("Service"))?
        .append1("network_manager.service");

    let connection = Connection::new_system()
        .map_err(|_| NetworkManagerError::Generic("Service"))?;

    let response = connection
        .send_with_reply_and_block(message, Duration::from_millis(2000))
        .map_err(|_| NetworkManagerError::Generic("Service"))?;

    let path = response
        .get1::<Path>()
        .ok_or(NetworkManagerError::Generic("Service"))?;

    let p = connection.with_proxy(SD_SERVICE_MANAGER, path, Duration::from_millis(2000));
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;

    let response: String = p.get(SD_UNIT_INTERFACE, "ActiveState")
        .map_err(|_| NetworkManagerError::Generic("Service"))?;

    Ok(response.parse().map_err(|_| NetworkManagerError::Generic("Service"))?)
}

fn handler(timeout: u64, target_state: ServiceState) -> Result<ServiceState, NetworkManagerError> {
    if timeout == 0 {
        return get_service_state();
    }

    let timer = Timer::default()
        .sleep(Duration::from_secs(timeout))
        .then(|_| return Err(NetworkManagerError::Generic("Service"));;);

    let process = CpuPool::new(1).spawn_fn(|| {
        let connection = Connection::new_system()
            .map_err(|_| NetworkManagerError::Generic("Service"))?;
        let proxy = connection.with_proxy("org.freedesktop.systemd1", "/org/freedesktop/systemd1/unit/NetworkManager_2eservice", Duration::from_millis(5000));

        let rule = MatchRule::new_signal();
        proxy.match_start(rule, true, |f| {

            check_received_signal(response)?
        })?;

        proxy.add_match(
            "type='signal', sender='org.freedesktop.systemd1', \
                 interface='org.freedesktop.DBus.Properties', \
                 member='PropertiesChanged', \
                 path='/org/freedesktop/systemd1/unit/NetworkManager_2eservice'",
        )
            .map_err(|_| NetworkManagerError::Generic("Service"))?;

        connection
            .add_match(
                "type='signal', sender='org.freedesktop.systemd1', \
                 interface='org.freedesktop.DBus.Properties', \
                 member='PropertiesChanged', \
                 path='/org/freedesktop/systemd1/unit/NetworkManager_2eservice'",
            )
            .map_err(|_| NetworkManagerError::Generic("Service"))?;

        if get_service_state()? == target_state {
            return Ok(target_state);
        }

        for item in connection.iter(0) {
            let response = if let ConnectionItem::Signal(ref signal) = item {
                signal
            } else {
                continue;
            };

            check_received_signal(response)?;
        }
        return Err(NetworkManagerError::Generic("Service"));
    });

    match timer.select(process).map(|(result, _)| result).wait() {
        Ok(val) => Ok(val),
        Err(val) => Err(val.0),
    }
}

fn check_received_signal(response: &Message) -> Result<(), NetworkManagerError> {
    if response
        .interface()
        .ok_or(NetworkManagerError::Generic("Service"))?
        != "org.freedesktop.DBus.Properties".into()
        || response
        .member()
        .ok_or(NetworkManagerError::Generic("Service"))?
        != "PropertiesChanged".into()
        || response
        .path()
        .ok_or(NetworkManagerError::Generic("Service"))?
        != "/org/freedesktop/systemd1/unit/NetworkManager_2eservice".into()
    {
        return Ok(());
    }

    let (interface, dictionary) = response.get2::<&str, Dict<&str, Variant<Iter>, _>>();

    if interface.ok_or(NetworkManagerError::Generic("Service"))?
        != "org.freedesktop.systemd1.Unit"
    {
        return Ok(());
    }

    for (k, mut v) in dictionary.ok_or(NetworkManagerError::Generic("Service"))? {
        if k == "ActiveState" {
            let response =
                v.0.get::<&str>()
                    .ok_or(NetworkManagerError::Generic("Service"))?;
            let state: ServiceState = response.parse()?;
            if state == target_state {
                return Ok(target_state);
            }
        }
    }

    return Ok(());
}

#[derive(Debug, Eq, PartialEq)]
pub enum ServiceState {
    Active,
    Reloading,
    Inactive,
    Failed,
    Activating,
    Deactivating,
}

impl FromStr for ServiceState {
    type Err = NetworkManagerError;
    fn from_str(s: &str) -> Result<ServiceState, NetworkManagerError> {
        match s {
            "active" => Ok(ServiceState::Active),
            "reloading" => Ok(ServiceState::Reloading),
            "inactive" => Ok(ServiceState::Inactive),
            "failed" => Ok(ServiceState::Failed),
            "activating" => Ok(ServiceState::Activating),
            "deactivating" => Ok(ServiceState::Deactivating),
            _ => Err(NetworkManagerError::Generic("Service")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_stop_service() {
        let s = get_service_state().unwrap();

        assert!(s == ServiceState::Active || s == ServiceState::Inactive);

        match s {
            ServiceState::Active => {
                stop_service(10).unwrap();
                assert_eq!(ServiceState::Inactive, get_service_state().unwrap());

                start_service(10).unwrap();
                assert_eq!(ServiceState::Active, get_service_state().unwrap());
            }
            ServiceState::Inactive => {
                start_service(10).unwrap();
                assert_eq!(ServiceState::Active, get_service_state().unwrap());

                stop_service(10).unwrap();
                assert_eq!(ServiceState::Inactive, get_service_state().unwrap());
            }
            _ => (),
        }
    }
}
