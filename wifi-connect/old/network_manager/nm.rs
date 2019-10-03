use super::{
    dbus::DBusApi, Connectivity, Device, NetworkManagerError, NetworkManagerState,
    METHOD_RETRY_ERROR_NAMES, NM_SERVICE_INTERFACE, NM_SERVICE_MANAGER, NM_SERVICE_PATH,
};

use crate::network_manager::device::get_devices;
use std::rc::Rc;
use crate::network_manager::DeviceType;

#[derive(Clone)]
pub struct NetworkManager {
    pub(crate) dbus: Rc<DBusApi>,
}

impl NetworkManager {
    pub fn new() -> Self {
        NetworkManager {
            dbus: Rc::new(DBusApi::new(
                NM_SERVICE_MANAGER,
                METHOD_RETRY_ERROR_NAMES,
                None,
            )),
        }
    }

    pub fn with_method_timeout(method_timeout: u64) -> Self {
        NetworkManager {
            dbus: Rc::new(DBusApi::new(
                NM_SERVICE_MANAGER,
                METHOD_RETRY_ERROR_NAMES,
                Some(method_timeout),
            )),
        }
    }

    pub fn method_timeout(&self) -> u64 {
        self.dbus.method_timeout()
    }

    pub fn get_state(&self) -> Result<NetworkManagerState, NetworkManagerError> {
        let response = self
            .dbus
            .call(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "state")?;

        let state: u32 = self.dbus.extract(&response)?;

        Ok(NetworkManagerState::from(state))
    }

    pub fn check_connectivity(&self) -> Result<Connectivity, NetworkManagerError> {
        let response =
            self.dbus
                .call(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "CheckConnectivity")?;

        let connectivity: u32 = self.dbus.extract(&response)?;

        Ok(Connectivity::from(connectivity))
    }

    pub fn is_wireless_enabled(&self) -> Result<bool, NetworkManagerError> {
        self.dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "WirelessEnabled")
    }

    pub fn set_wireless_enabled(&self, enabled: bool) -> Result<bool, NetworkManagerError> {
        self.dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "WirelessEnabled")
    }

    pub fn is_networking_enabled(&self) -> Result<bool, NetworkManagerError> {
        self.dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "NetworkingEnabled")
    }

    pub fn get_devices(&self) -> Result<Vec<Device>, NetworkManagerError> {
        get_devices(self)
    }

    /// Convenience method to only enumerate wifi devices
    pub fn get_wifi_devices(&self) -> Result<Vec<Device>, NetworkManagerError> {
        Ok(get_devices(self)?
            .into_iter()
            .filter(|device| *device.device_type() == DeviceType::WiFi)
            .collect())
    }

    /// Convenience method to only enumerate ethernet devices
    pub fn get_ethernet_devices(&self) -> Result<Vec<Device>, NetworkManagerError> {
        Ok(get_devices(self)?
            .into_iter()
            .filter(|device| *device.device_type() == DeviceType::Ethernet)
            .collect())
    }
}
