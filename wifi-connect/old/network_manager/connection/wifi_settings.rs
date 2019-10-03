use super::super::{
    dbus::{extract, variant_iter_to_vec_u8},
    NetworkManager, NetworkManagerError, NM_CONNECTION_INTERFACE,
};
use super::Ssid;
use dbus::arg::{Dict, Iter, Variant};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WiFiConnectionSettings {
    pub kind: String,
    // `type` is nm_dbus_generated reserved word, so we are using `kind` instead
    pub id: String,
    pub uuid: String,
    /// According to last standard 802.11-2012 (Section 6.3.11.2.2),
    /// nm_dbus_generated SSID  can be 0-32 octets with an unspecified or UTF8 encoding.
    pub ssid: Ssid,
    pub mode: String,
}

pub fn get_connection_settings(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<WiFiConnectionSettings, NetworkManagerError> {
    let response = dbus_manager
        .dbus
        .call(path, NM_CONNECTION_INTERFACE, "GetSettings")?;

    let dict: Dict<&str, Dict<&str, Variant<Iter>, _>, _> = dbus_manager.dbus.extract(&response)?;

    let mut kind = String::new();
    let mut id = String::new();
    let mut uuid = String::new();
    let mut ssid = Ssid::new();
    let mut mode = String::new();

    for (_, v1) in dict {
        for (k2, mut v2) in v1 {
            match k2 {
                "id" => {
                    id = extract::<String>(&mut v2)?;
                },
                "uuid" => {
                    uuid = extract::<String>(&mut v2)?;
                },
                "type" => {
                    kind = extract::<String>(&mut v2)?;
                },
                "ssid" => {
                    ssid = Ssid::from_bytes(variant_iter_to_vec_u8(&mut v2)?);
                },
                "mode" => {
                    mode = extract::<String>(&mut v2)?;
                },
                _ => {},
            }
        }
    }

    Ok(WiFiConnectionSettings {
        kind,
        id,
        uuid,
        ssid,
        mode,
    })
}
