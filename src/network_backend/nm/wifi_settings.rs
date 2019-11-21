//! A network manager dbus API wifi connection does not have convenient properties for all its
//! settings. Instead settings are submitted and retrieved in a generic HashMap (which contains
//! dbus crate Variants and VariantMaps).
//!
//! This module creates and encodes those data containers.
//! This is an internal implementation detail of the network manager implementation.

use super::NM_BUSNAME;
use crate::network_interface::{AccessPointCredentials, SSID};
use crate::utils::verify_password;
use crate::CaptivePortalError;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

use dbus::arg::{RefArg, Variant};
use dbus::{nonblock, nonblock::SyncConnection};

const NM_WEP_KEY_TYPE_PASSPHRASE: u8 = 2;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WifiConnectionMode {
    AP,
    Infrastructure,
    AdHoc,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct WiFiConnectionSettings {
    pub id: String,
    pub uuid: String,
    pub ssid: SSID,
    pub mode: WifiConnectionMode,
    pub seen_bssids: Vec<String>,
}

/**
Network manager example output:

{'802-11-wireless': {'mac-address-blacklist': [],
                     'mode': 'ap',
                     'security': '802-11-wireless-security',
                     'seen-bssids': ['30:52:CB:84:B5:B5'],
                     'ssid': [100,
                              97,
                              118,
                              105,
                              100,
                              108,
                              97,
                              112,
                              116,
                              111,
                              112]},
 '802-11-wireless-security': {'group': ['ccmp'],
                              'key-mgmt': 'wpa-psk',
                              'pairwise': ['ccmp'],
                              'proto': ['rsn']},
 'connection': {'autoconnect': False,
                'id': 'Hotspot',
                'permissions': [],
                'timestamp': 1570824896,
                'type': '802-11-wireless',
                'uuid': 'a2f7487b-cb73-42f1-88ec-38325584736b'},
 'ipv4': {'address-data': [],
          'addresses': [],
          'dns': [],
          'dns-search': [],
          'method': 'shared',
          'route-data': [],
          'routes': []},
 'ipv6': {'address-data': [],
          'addresses': [],
          'dns': [],
          'dns-search': [],
          'method': 'auto',
          'route-data': [],
          'routes': []},
 'proxy': {}}
*/
pub(crate) fn make_arguments_for_sta(
    ssid: SSID,
    password: Option<String>,
    address: Option<Ipv4Addr>,
    interface: &str,
    uuid: &str,
) -> Result<HashMap<&'static str, VariantMap>, CaptivePortalError> {
    let mut settings: HashMap<&'static str, VariantMap> = HashMap::new();

    let mut wireless: VariantMap = HashMap::new();
    add_val(&mut wireless, "ssid", ssid.as_bytes().to_owned());
    add_str(&mut wireless, "band", "bg");
    add_val(&mut wireless, "hidden", false);
    add_str(&mut wireless, "mode", "ap");
    if let Some(password) = password {
        add_str(&mut wireless, "security", "802-11-wireless-security");

        let mut security: VariantMap = HashMap::new();
        add_str(&mut security, "key-mgmt", "wpa-psk");
        add_str(&mut security, "psk", &verify_password(password)?);

        settings.insert("802-11-wireless-security", security);
    }
    settings.insert("802-11-wireless", wireless);

    // See https://developer.gnome.org/NetworkManager/stable/nm-settings.html
    let mut connection: VariantMap = HashMap::new();
    add_str(&mut connection, "id", "Hotspot");
    add_str(&mut connection, "interface-name", interface);
    add_str(&mut connection, "uuid", uuid);
    add_str(&mut connection, "type", "802-11-wireless");
    add_val(&mut connection, "autoconnect", false);
    settings.insert("connection", connection);

    let mut ipv4: VariantMap = HashMap::new();
    if let Some(address) = address {
        add_str(&mut ipv4, "method", "manual");

        let mut addr_map: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        addr_map.insert("address".to_owned(), Variant(Box::new(format!("{}", address))));
        addr_map.insert("prefix".to_owned(), Variant(Box::new(24_u32)));
        add_val(&mut ipv4, "address-data", vec![addr_map]);
    } else {
        add_str(&mut ipv4, "method", "shared");
    }
    settings.insert("ipv4", ipv4);

    Ok(settings)
}

/// The connection should be temporary only, until explicitly saved.
pub(crate) fn make_options_for_ap() -> HashMap<&'static str, Variant<Box<dyn RefArg>>> {
    let mut options = HashMap::new();
    // * persist: A string value of either "disk" (default), "memory" or "volatile".
    // If "memory" is passed, the connection will not be saved to disk.
    // If "volatile" is passed, the connection will not be saved to disk and will be destroyed when disconnected.
    add_str(&mut options, "persist", "volatile");
    options
}

pub(crate) fn make_arguments_for_ap<T: Eq + std::hash::Hash + std::convert::From<&'static str>>(
    ssid: &SSID,
    credentials: AccessPointCredentials,
    old_connection: Option<WiFiConnectionSettings>,
) -> Result<HashMap<T, VariantMap>, CaptivePortalError> {
    let mut settings: HashMap<T, VariantMap> = HashMap::new();

    let mut wireless: VariantMap = HashMap::new();
    add_val(&mut wireless, "ssid", ssid.as_bytes().to_owned());
    settings.insert("802-11-wireless".into(), wireless);

    let mut connection: VariantMap = HashMap::new();
    // See https://developer.gnome.org/NetworkManager/stable/nm-settings.html
    add_val(&mut connection, "autoconnect", true);
    if let Some(old_connection) = old_connection {
        add_val(&mut connection, "id", old_connection.id);
        add_val(&mut connection, "uuid", old_connection.uuid);
    }
    settings.insert("connection".into(), connection);

    prepare_wifi_security_settings(&credentials, &mut settings)?;

    Ok(settings)
}

/// Adds necessary entries to the given settings map.
/// To be used by wifi device connect and [`add_wifi_connection`].
pub(crate) fn prepare_wifi_security_settings<T: Eq + std::hash::Hash + std::convert::From<&'static str>>(
    credentials: &AccessPointCredentials,
    settings: &mut HashMap<T, VariantMap>,
) -> Result<(), CaptivePortalError> {
    match *credentials {
        AccessPointCredentials::Wep { ref passphrase } => {
            let mut security_settings: VariantMap = HashMap::new();

            add_val(&mut security_settings, "wep-key-type", NM_WEP_KEY_TYPE_PASSPHRASE);
            add_val(&mut security_settings, "wep-key0", verify_password(passphrase.clone())?);

            settings.insert("802-11-wireless-security".into(), security_settings);
        },
        AccessPointCredentials::Wpa { ref passphrase } => {
            let mut security_settings: VariantMap = HashMap::new();

            add_str(&mut security_settings, "key-mgmt", "wpa-psk");
            add_val(&mut security_settings, "psk", verify_password(passphrase.clone())?);

            settings.insert("802-11-wireless-security".into(), security_settings);
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

            settings.insert("802-11-wireless-security".into(), security_settings);
            settings.insert("802-1x".into(), eap);
        },
        AccessPointCredentials::None => {},
    };
    Ok(())
}

pub(crate) fn extract(key: &str, map: &HashMap<String, Variant<Box<dyn RefArg>>>) -> String {
    map.get(key)
        .and_then(|v| v.0.as_str().and_then(|v| Some(v.to_owned())))
        .unwrap_or_default()
}

pub(crate) fn extract_bytes(key: &str, map: &HashMap<String, Variant<Box<dyn RefArg>>>) -> Vec<u8> {
    map.get(key)
        .and_then(|v| v.0.as_iter())
        .and_then(|v| {
            Some(
                v.filter_map(|v| match v.as_u64() {
                    Some(v) => Some(v as u8),
                    None => None,
                })
                .collect(),
            )
        })
        .unwrap_or_default()
}

pub(crate) fn extract_vector(key: &str, map: &HashMap<String, Variant<Box<dyn RefArg>>>) -> Vec<String> {
    map.get(key)
        .and_then(|v| v.0.as_iter())
        .and_then(|v| {
            Some(
                v.filter_map(|v| match v.as_str() {
                    Some(v) => Some(v.to_owned()),
                    None => None,
                })
                .collect(),
            )
        })
        .unwrap_or_default()
}

/// Return a wifi connection settings object if the given connection (or active connection) is a wifi connection and None otherwise.
pub(crate) async fn get_connection_settings(
    conn: Arc<SyncConnection>,
    connection_path: dbus::Path<'_>,
) -> Result<Option<WiFiConnectionSettings>, CaptivePortalError> {
    // The api consumer might hand us an active connection instead of a regular one. If so, determine the connection path
    // and overwrite the proxy.
    let mut p = nonblock::Proxy::new(NM_BUSNAME, connection_path.clone(), conn.clone());
    if connection_path.clone().to_string().contains("ActiveConnection") {
        use super::generated::connection_active::ConnectionActive;
        let path = p.connection().await?;
        p = nonblock::Proxy::new(NM_BUSNAME, path, conn.clone());
    };

    use super::generated::connection_nm::Connection;

    let dict = p.get_settings().await?;

    let wireless_settings = if let Some(v) = dict.get("802-11-wireless") {
        v
    } else {
        return Ok(None);
    };
    let connection_settings = if let Some(v) = dict.get("connection") {
        v
    } else {
        return Ok(None);
    };

    let mode = match &extract("mode", &wireless_settings)[..] {
        "ap" => WifiConnectionMode::AP,
        "adhoc" => WifiConnectionMode::AdHoc,
        "infrastructure" => WifiConnectionMode::Infrastructure,
        s => {
            warn!(
                "Wifi connection setting without known mode found: {}. Assuming infrastructure.",
                s
            );
            WifiConnectionMode::Infrastructure
        },
    };

    let d = extract_bytes("ssid", &wireless_settings);

    Ok(Some(WiFiConnectionSettings {
        id: extract("id", &connection_settings),
        uuid: extract("uuid", &connection_settings),
        ssid: String::from_utf8(d)?,
        mode,
        seen_bssids: extract_vector("seen-bssids", &wireless_settings),
    }))
}

/// Dbus library helper type
pub(crate) type VariantMap = HashMap<&'static str, Variant<Box<dyn RefArg>>>;
pub(crate) type VariantMapNested = HashMap<&'static str, HashMap<&'static str, Variant<Box<dyn RefArg>>>>;

pub(crate) fn add_val<V>(map: &mut VariantMap, key: &'static str, value: V)
where
    V: RefArg + 'static,
{
    map.insert(key, Variant(Box::new(value)));
}

pub(crate) fn add_str<V>(map: &mut VariantMap, key: &'static str, value: V)
where
    V: Into<String>,
{
    map.insert(key, Variant(Box::new(value.into())));
}
