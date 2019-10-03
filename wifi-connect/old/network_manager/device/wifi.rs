use super::super::{add_val, VariantMap};
use super::{
    connection::{
        add_wifi_connection, prepare_wifi_security_settings, wait_for_connection, Connection,
        ConnectionState,
    },
    path_to_string,
    wifi_ap::{
        get_access_point, get_device_access_points, request_access_point_scan, AccessPoint,
        AccessPointCredentials,
    },
    Device, NetworkManagerError, PathGetter, NM_SERVICE_INTERFACE, NM_SERVICE_PATH,
};

use crate::network_manager::{create_hotspot, Ssid};
use dbus::arg::RefArg;
use dbus::Path;
use std::collections::HashMap;
use std::net::Ipv4Addr;

pub trait WiFiDevice {
    /// Get the list of access points visible to this device.
    ///
    /// # Examples
    ///
    /// ```
    /// use network_manager::{NetworkManager, DeviceType};
    /// let manager = NetworkManager::new();
    /// let devices = manager.get_devices().unwrap();
    /// let i = devices.iter().position(|ref d| *d.device_type() == DeviceType::WiFi).unwrap();
    /// let device = devices[i].as_wifi_device().unwrap();
    /// device.request_scan()?;
    /// let access_points = device.get_access_points().unwrap();
    /// println!("{:?}", access_points);
    /// ```
    fn get_access_points(&self) -> Result<Vec<AccessPoint>, NetworkManagerError>;

    fn find_access_point<'b>(&self, ssid: &Ssid) -> Option<AccessPoint>;

    fn request_scan(&self) -> Result<(), NetworkManagerError>;

    fn create_hotspot(
        &self,
        ssid: Ssid,
        password: Option<String>,
        address: Option<Ipv4Addr>,
    ) -> Result<(Connection, ConnectionState), NetworkManagerError>;

    fn add_connection(
        &self,
        ssid: &str,
        credentials: &AccessPointCredentials,
    ) -> Result<Connection, NetworkManagerError>;

    fn connect(
        &self,
        access_point: &AccessPoint,
        credentials: &AccessPointCredentials,
    ) -> Result<(Connection, ConnectionState), NetworkManagerError>;
}

impl WiFiDevice for Device {
    fn get_access_points(&self) -> Result<Vec<AccessPoint>, NetworkManagerError> {
        let mut access_points = Vec::new();

        let paths = get_device_access_points(&self.dbus_manager, self.path())?;

        for path in paths {
            if let Some(access_point) = get_access_point(&self.dbus_manager, &path)? {
                access_points.push(access_point);
            }
        }

        access_points.sort_by_key(|ap| ap.strength);
        access_points.reverse();

        Ok(access_points)
    }

    fn find_access_point<'b>(&self, ssid: &Ssid) -> Option<AccessPoint> {
        for access_point in self.get_access_points().ok()?.into_iter() {
            if &access_point.ssid == ssid {
                return Some(access_point);
            }
        }
        None
    }

    fn request_scan(&self) -> Result<(), NetworkManagerError> {
        request_access_point_scan(&self.dbus_manager, self.path())?;
        Ok(())
    }

    fn create_hotspot(
        &self,
        ssid: Ssid,
        password: Option<String>,
        address: Option<Ipv4Addr>,
    ) -> Result<(Connection, ConnectionState), NetworkManagerError> {
        create_hotspot(
            &self.dbus_manager,
            self.path(),
            self.interface(),
            ssid,
            password,
            address,
        )
    }

    fn add_connection(
        &self,
        ssid: &str,
        credentials: &AccessPointCredentials,
    ) -> Result<Connection, NetworkManagerError> {
        add_wifi_connection(
            &self.dbus_manager,
            ssid,
            &self.interface(),
            credentials,
        )
    }

    fn connect(
        &self,
        access_point: &AccessPoint,
        credentials: &AccessPointCredentials,
    ) -> Result<(Connection, ConnectionState), NetworkManagerError> {
        let mut settings: HashMap<String, VariantMap> = HashMap::new();

        let mut wireless: VariantMap = HashMap::new();
        add_val(&mut wireless, "ssid", access_point.ssid.data().clone());
        settings.insert("802-11-wireless".to_string(), wireless);

        prepare_wifi_security_settings(credentials, &mut settings)?;

        let response = self.dbus_manager.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "AddAndActivateConnection",
            &[
                &settings as &dyn RefArg,
                &Path::new(&self.path as &str)? as &dyn RefArg,
                &Path::new(&access_point.path as &str)? as &dyn RefArg,
            ],
        )?;

        let (conn_path, _active_connection): (Path, Path) =
            self.dbus_manager.dbus.extract_two(&response)?;
        let conn_path = path_to_string(&conn_path)?;

        let connection = Connection::init(self.dbus_manager.clone(), conn_path)?;

        let state = wait_for_connection(
            &connection,
            &ConnectionState::Activated,
            self.dbus_manager.method_timeout(),
        )?;

        Ok((connection, state))
    }
}
