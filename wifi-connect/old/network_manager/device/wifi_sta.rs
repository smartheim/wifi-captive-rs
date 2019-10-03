use super::super::*;
use crate::network_manager::connection::wait_for_connection;
use crate::network_manager::dbus::path_to_string;
use ::dbus::arg::RefArg;
use ::dbus::Path;
use std::collections::HashMap;
use std::net::Ipv4Addr;

pub fn create_hotspot(
    dbus_manager: &NetworkManager,
    device_path: &str,
    interface: &str,
    ssid: Ssid,
    password: Option<String>,
    address: Option<Ipv4Addr>,
) -> Result<(Connection, ConnectionState), NetworkManagerError> {
    let ssid_user_string = ssid.to_string();

    let mut wireless: VariantMap = HashMap::new();
    add_val(&mut wireless, "ssid", ssid.into_vec());
    add_str(&mut wireless, "band", "bg");
    add_val(&mut wireless, "hidden", false);
    add_str(&mut wireless, "mode", "ap");

    let mut connection: VariantMap = HashMap::new();
    add_val(&mut connection, "autoconnect", false);
    if let Ok(ssid_str) = ssid_user_string {
        add_str(&mut connection, "id", ssid_str);
    }
    add_str(&mut connection, "interface-name", interface);
    add_str(&mut connection, "type", "802-11-wireless");

    let mut ipv4: VariantMap = HashMap::new();
    if let Some(address) = address {
        add_str(&mut ipv4, "method", "manual");

        let mut addr_map: VariantMap = HashMap::new();
        add_str(&mut addr_map, "address", format!("{}", address));
        add_val(&mut addr_map, "prefix", 24_u32);

        add_val(&mut ipv4, "address-data", vec![addr_map]);
    } else {
        add_str(&mut ipv4, "method", "shared");
    }

    let mut settings: HashMap<String, VariantMap> = HashMap::new();

    if let Some(password) = password {
        add_str(&mut wireless, "security", "802-11-wireless-security");

        let mut security: VariantMap = HashMap::new();
        add_str(&mut security, "key-mgmt", "wpa-psk");
        add_string(&mut security, "psk", verify_ascii_password(password)?);

        settings.insert("802-11-wireless-security".to_string(), security);
    }

    settings.insert("802-11-wireless".to_string(), wireless);
    settings.insert("connection".to_string(), connection);
    settings.insert("ipv4".to_string(), ipv4);

    let response = dbus_manager.dbus.call_with_args(
        NM_SERVICE_PATH,
        NM_SERVICE_INTERFACE,
        "AddAndActivateConnection",
        &[
            &settings as &dyn RefArg,
            &Path::new(device_path)? as &dyn RefArg,
            &Path::new("/")? as &dyn RefArg,
        ],
    )?;

    let (conn_path, _active_connection): (Path, Path) = dbus_manager.dbus.extract_two(&response)?;
    let conn_path = path_to_string(&conn_path)?;
    let connection = Connection::init(dbus_manager.clone(), conn_path)?;

    let state = wait_for_connection(
        &connection,
        &ConnectionState::Activated,
        dbus_manager.method_timeout(),
    )?;

    Ok((connection, state))
}
