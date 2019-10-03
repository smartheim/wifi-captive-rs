pub mod ethernet;
pub mod state;
pub mod wifi;
pub mod wifi_ap;
pub mod wifi_sta;

use ethernet::EthernetDevice;
use state::{DeviceState, DeviceType};
use wifi::WiFiDevice;

use std::fmt;

use super::connection;
use super::NetworkManager;
use super::NetworkManagerError;
use super::{NM_DEVICE_INTERFACE, NM_SERVICE_INTERFACE, NM_SERVICE_PATH};
use crate::network_manager::dbus::path_to_string;
use dbus::arg::RefArg;
use dbus::Path;

#[derive(Clone)]
pub struct Device {
    dbus_manager: NetworkManager,
    path: String,
    interface: String,
    device_type: DeviceType,
}

pub trait PathGetter {
    fn path(&self) -> &str;
}

impl PathGetter for Device {
    fn path(&self) -> &str {
        &self.path
    }
}

impl Device {
    pub fn try_new(
        dbus_manager: NetworkManager,
        path: String,
    ) -> Result<Self, NetworkManagerError> {
        let interface: String =
            dbus_manager
                .dbus
                .property(&path, NM_DEVICE_INTERFACE, "Interface")?;
        let device_type: DeviceType =
            dbus_manager
                .dbus
                .property(&path, NM_DEVICE_INTERFACE, "DeviceType")?;

        Ok(Device {
            dbus_manager,
            path,
            interface,
            device_type,
        })
    }

    pub fn by_interface(
        dbus_manager: NetworkManager,
        interface: &str,
    ) -> Result<Device, NetworkManagerError> {
        let response = dbus_manager.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "GetDeviceByIpIface",
            &[&interface.to_string() as &dyn RefArg],
        )?;

        let path: Path = dbus_manager.dbus.extract(&response)?;
        let path = path_to_string(&path)?;
        Device::try_new(dbus_manager, path)
    }

    pub fn device_type(&self) -> &DeviceType {
        &self.device_type
    }

    pub fn interface(&self) -> &str {
        &self.interface
    }

    pub fn as_wifi_device(&self) -> Option<&dyn WiFiDevice> {
        if self.device_type == DeviceType::WiFi {
            Some(self as &dyn WiFiDevice)
        } else {
            None
        }
    }

    pub fn as_ethernet_device(&self) -> Option<&dyn EthernetDevice> {
        if self.device_type == DeviceType::Ethernet {
            Some(self as &dyn EthernetDevice)
        } else {
            None
        }
    }

    pub fn state(&self) -> Result<DeviceState, NetworkManagerError> {
        self.dbus_manager
            .dbus
            .property(&self.path, NM_DEVICE_INTERFACE, "State")
    }

    /// Connects nm_dbus_generated Network Manager device.
    ///
    /// Examples
    ///
    /// ```
    /// use network_manager::{NetworkManager, DeviceType};
    /// let manager = NetworkManager::new();
    /// let devices = manager.get_devices().unwrap();
    /// let i = devices.iter().position(|ref d| *d.device_type() == DeviceType::WiFi).unwrap();
    /// devices[i].connect().unwrap();
    /// ```
    pub fn connect(&self) -> Result<DeviceState, NetworkManagerError> {
        let state = self.state()?;

        match state {
            DeviceState::Activated => Ok(DeviceState::Activated),
            _ => {
                self.dbus_manager.dbus.call_with_args(
                    NM_SERVICE_PATH,
                    NM_SERVICE_INTERFACE,
                    "ActivateConnection",
                    &[
                        &Path::new("/")? as &dyn RefArg,
                        &Path::new(&self.path as &str)? as &dyn RefArg,
                        &Path::new("/")? as &dyn RefArg,
                    ],
                )?;

                wait_for_device(
                    self,
                    &DeviceState::Activated,
                    self.dbus_manager.method_timeout(),
                )
            }
        }
    }

    /// Disconnect nm_dbus_generated Network Manager device.
    ///
    /// # Examples
    ///
    /// ```
    /// use network_manager::{NetworkManager, DeviceType};
    /// let manager = NetworkManager::new();
    /// let devices = manager.get_devices().unwrap();
    /// let i = devices.iter().position(|ref d| *d.device_type() == DeviceType::WiFi).unwrap();
    /// devices[i].disconnect().unwrap();
    /// ```
    pub fn disconnect(&self) -> Result<DeviceState, NetworkManagerError> {
        let state = self.state()?;

        match state {
            DeviceState::Disconnected => Ok(DeviceState::Disconnected),
            _ => {
                self.dbus_manager
                    .dbus
                    .call(&self.path, NM_DEVICE_INTERFACE, "Disconnect")?;

                wait_for_device(
                    self,
                    &DeviceState::Disconnected,
                    self.dbus_manager.method_timeout(),
                )
            }
        }
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Device {{ path: {:?}, interface: {:?}, device_type: {:?} }}",
            self.path, self.interface, self.device_type
        )
    }
}

/// Get nm_dbus_generated list of Network Manager devices.
///
/// # Examples
///
/// ```
/// use network_manager::NetworkManager;
/// let manager = NetworkManager::new();
/// let devices = manager.get_devices().unwrap();
/// println!("{:?}", devices);
/// ```
pub fn get_devices(dbus_manager: &NetworkManager) -> Result<Vec<Device>, NetworkManagerError> {
    let device_paths: Vec<String> =
        dbus_manager
            .dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "Devices")?;

    let mut result = Vec::with_capacity(device_paths.len());

    for path in device_paths {
        let device = Device::try_new(dbus_manager.clone(), path)?;

        result.push(device);
    }

    Ok(result)
}

pub fn get_active_connection_devices(
    dbus_manager: &NetworkManager,
    active_path: &str,
) -> Result<Vec<Device>, NetworkManagerError> {
    let device_paths =
        super::connection::get_active_connection_device_paths(dbus_manager, active_path)?;

    let mut result = Vec::with_capacity(device_paths.len());

    for path in device_paths {
        let device = Device::try_new(dbus_manager.clone(), path)?;
        result.push(device);
    }

    Ok(result)
}

pub(crate) fn wait_for_device(
    device: &Device,
    target_state: &DeviceState,
    timeout: u64,
) -> Result<DeviceState, NetworkManagerError> {
    if timeout == 0 {
        return device.state();
    }

    debug!("Waiting for device state: {:?}", target_state);

    let mut total_time = 0;

    loop {
        ::std::thread::sleep(::std::time::Duration::from_secs(1));

        let state = device.state()?;

        total_time += 1;

        if state == *target_state {
            debug!(
                "Device target state reached: {:?} / {}s elapsed",
                state, total_time
            );

            return Ok(state);
        } else if total_time >= timeout {
            debug!(
                "Timeout reached in waiting for device state ({:?}): {:?} / {}s elapsed",
                target_state, state, total_time
            );

            return Ok(state);
        }

        debug!(
            "Still waiting for device state ({:?}): {:?} / {}s elapsed",
            target_state, state, total_time
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::{device::get_devices, NetworkManager};

    use super::*;

    #[test]
    fn test_device_connect_disconnect() {
        let manager = NetworkManager::new();

        let devices = get_devices(&manager).unwrap();

        let i = devices
            .iter()
            .position(|ref d| d.device_type == DeviceType::WiFi)
            .unwrap();
        let device = &devices[i];

        let state = device.state().unwrap();

        if state == DeviceState::Activated {
            let state = device.disconnect().unwrap();
            assert_eq!(DeviceState::Disconnected, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));

            let state = device.connect().unwrap();
            assert_eq!(DeviceState::Activated, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));
        } else {
            let state = device.connect().unwrap();
            assert_eq!(DeviceState::Activated, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));

            let state = device.disconnect().unwrap();
            assert_eq!(DeviceState::Disconnected, state);

            ::std::thread::sleep(::std::time::Duration::from_secs(5));
        }
    }
}
