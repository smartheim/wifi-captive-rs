use super::{Device};
use dbus::arg::RefArg;
use dbus::Path;
use std::collections::HashMap;
use std::net::Ipv4Addr;

use super::super::{
    add_str, add_val, connection::wait_for_connection, dbus::path_to_string, Connection,
    ConnectionState, NetworkManagerError, VariantMap, NM_SERVICE_INTERFACE, NM_SERVICE_PATH,
};

pub trait EthernetDevice {
    fn connect(
        &self,
        address: Ipv4Addr,
        address_netmask_bit_count: u8,
        gateway: Ipv4Addr,
        dns_addr_1: Ipv4Addr,
        dns_addr_2: Ipv4Addr,
        dns_search: &str,
        method: &str,
        connection_name: &str,
    ) -> Result<(Connection, ConnectionState), NetworkManagerError>;
}

impl EthernetDevice for Device {
    fn connect(
        &self,
        address: Ipv4Addr,
        address_netmask_bit_count: u8,
        gateway: Ipv4Addr,
        dns_addr_1: Ipv4Addr,
        dns_addr_2: Ipv4Addr,
        dns_search: &str,
        method: &str,
        connection_name: &str,
    ) -> Result<(Connection, ConnectionState), NetworkManagerError> {
        let mut connection: VariantMap = HashMap::new();
        add_str(&mut connection, "id", connection_name);
        add_str(&mut connection, "interface-name", &self.interface);
        add_str(&mut connection, "type", "802-3-ethernet");

        let mut ipv4: VariantMap = HashMap::new();
        add_str(&mut ipv4, "method", method);

        if method == "manual" {
            add_str(&mut ipv4, "gateway", format!("{}", gateway));
            let mut addr_map: VariantMap = HashMap::new();
            let prefix = address_netmask_bit_count as u32;
            add_str(&mut addr_map, "address", format!("{}", address));
            add_val(&mut addr_map, "prefix", prefix);
            add_val(&mut ipv4, "address-data", vec![addr_map]);
        }
        add_val(
            &mut ipv4,
            "dns",
            vec![u32::from(dns_addr_1), u32::from(dns_addr_2)],
        );
        add_val(&mut ipv4, "dns-search", vec![dns_search.to_string()]);

        let mut settings: HashMap<String, VariantMap> = HashMap::new();
        settings.insert("connection".to_string(), connection);
        settings.insert("ipv4".to_string(), ipv4);

        let response = self.dbus_manager.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "AddAndActivateConnection",
            &[
                &settings as &dyn RefArg,
                &Path::new(&self.path as &str)? as &dyn RefArg,
                &Path::new("/")? as &dyn RefArg,
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
