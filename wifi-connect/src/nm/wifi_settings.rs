use super::{add_str, add_val, AccessPointCredentials, VariantMap, SSID, NM_CONNECTION_INTERFACE, security};
use crate::utils::verify_ascii_password;
use crate::CaptivePortalError;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use dbus::{
    nonblock,
    nonblock::SyncConnection,
    arg::Variant,
};
use dbus::arg::RefArg;

const NM_WEP_KEY_TYPE_PASSPHRASE: u8 = 2;

#[derive(Debug, Eq, PartialEq)]
pub enum WifiConnectionMode {
    AP,
    Infrastructure,
}

#[derive(Debug, Eq, PartialEq)]
pub struct WiFiConnectionSettings {
    pub id: String,
    pub uuid: String,
    pub ssid: SSID,
    pub mode: WifiConnectionMode,
    pub seen_bssids: Vec<String>,
}

pub(crate) fn make_arguments_for_sta(
    ssid: SSID,
    password: Option<String>,
    address: Option<Ipv4Addr>,
    interface: &str,
) -> Result<HashMap<&'static str, VariantMap>, CaptivePortalError> {
    let mut wireless: VariantMap = HashMap::new();
    add_val(&mut wireless, "ssid", ssid.clone());
    add_str(&mut wireless, "band", "bg");
    add_val(&mut wireless, "hidden", false);
    add_str(&mut wireless, "mode", "ap");

    let mut connection: VariantMap = HashMap::new();
    add_val(&mut connection, "autoconnect", false);
    add_val(&mut connection, "id", ssid);
    add_str(&mut connection, "interface-name", interface);
    add_str(&mut connection, "type", "802-11-wireless");

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

    let mut settings: HashMap<&'static str, VariantMap> = HashMap::new();

    if let Some(password) = password {
        add_str(&mut wireless, "security", "802-11-wireless-security");

        let mut security: VariantMap = HashMap::new();
        add_str(&mut security, "key-mgmt", "wpa-psk");
        add_val(&mut security, "psk", verify_ascii_password(password)?);

        settings.insert("802-11-wireless-security", security);
    }

    settings.insert("802-11-wireless", wireless);
    settings.insert("connection", connection);
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
    ssid: SSID,
    credentials: security::AccessPointCredentials,
) -> Result<HashMap<T, VariantMap>, CaptivePortalError> {
    let mut settings: HashMap<T, VariantMap> = HashMap::new();

    let mut wireless: VariantMap = HashMap::new();
    add_val(&mut wireless, "ssid", ssid);
    settings.insert("802-11-wireless".into(), wireless);

    let mut connection: VariantMap = HashMap::new();
    add_str(&mut connection, "auto-connect", "true");
    settings.insert("connection".into(), connection);

    prepare_wifi_security_settings(&credentials, &mut settings)?;

    Ok(settings)
}

/// Adds necessary entries to the given settings map.
/// To be used by wifi device connect and [`add_wifi_connection`].
pub fn prepare_wifi_security_settings<T: Eq + std::hash::Hash + std::convert::From<&'static str>>(
    credentials: &AccessPointCredentials,
    settings: &mut HashMap<T, VariantMap>,
) -> Result<(), CaptivePortalError> {
    match *credentials {
        AccessPointCredentials::Wep { ref passphrase } => {
            let mut security_settings: VariantMap = HashMap::new();

            add_val(
                &mut security_settings,
                "wep-key-type",
                NM_WEP_KEY_TYPE_PASSPHRASE,
            );
            add_val(
                &mut security_settings,
                "wep-key0",
                verify_ascii_password(passphrase.clone())?,
            );

            settings.insert("802-11-wireless-security".into(), security_settings);
        }
        AccessPointCredentials::Wpa { ref passphrase } => {
            let mut security_settings: VariantMap = HashMap::new();

            add_str(&mut security_settings, "key-mgmt", "wpa-psk");
            add_val(
                &mut security_settings,
                "psk",
                verify_ascii_password(passphrase.clone())?,
            );

            settings.insert("802-11-wireless-security".into(), security_settings);
        }
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
        }
        AccessPointCredentials::None => {}
    };
    Ok(())
}

pub fn extract(key: &str, map: &HashMap<String, Variant<Box<dyn RefArg>>>) -> String {
    map.get(key).and_then(|v| Some(v.as_str().unwrap().to_owned())).unwrap_or_default()
}

/// Return a wifi connection settings object if the given connection is a wifi connection and None otherwise.
pub async fn get_connection_settings(
    conn: Arc<SyncConnection>,
    connection_path: dbus::Path<'_>,
) -> Result<Option<WiFiConnectionSettings>, CaptivePortalError> {
    let p = nonblock::Proxy::new(NM_CONNECTION_INTERFACE, connection_path, conn.clone());
    use super::generated::connection_nm::OrgFreedesktopNetworkManagerSettingsConnection;

    let dict = p.get_settings().await?;

    let wireless_settings = dict.get("802-11-wireless");
    let connection_settings = dict.get("connection");
    if wireless_settings.is_none() || connection_settings.is_none() {
        return Ok(None);
    }
    let wireless_settings = wireless_settings.unwrap();
    let connection_settings = connection_settings.unwrap();

    // This monstrosity first extracts "seen-bssids" (if any otherwise a default empty vector is used).
    // The Variant is then casted into an iterator. Each entry is mapped to a String and in the end "collect"ed.
    let seen_bssids: Vec<String> = wireless_settings.get("seen-bssids")
        .and_then(|v| Some(v.as_iter().unwrap()
            .map(|v| v.as_str().unwrap().to_owned()).collect())).unwrap_or_default();

    let mode = match &extract("mode", &wireless_settings)[..] {
        "ap" => WifiConnectionMode::AP,
        "infrastructure" => WifiConnectionMode::Infrastructure,
        s => return Err(CaptivePortalError::OwnedString(format!("Wifi device mode not recognised: {}", s)))
    };

    Ok(Some(WiFiConnectionSettings {
        id: extract("id", &connection_settings),
        uuid: extract("uuid", &connection_settings),
        ssid: extract("ssid", &wireless_settings),
        mode,
        seen_bssids,
    }))
}
