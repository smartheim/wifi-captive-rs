use super::super::dbus::{DBusApi, VariantTo};
use super::super::*;
use ::dbus::arg::{RefArg, Variant};
use bitflags::bitflags;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AccessPoint {
    pub path: String,
    pub ssid: Ssid,
    pub strength: u32,
    pub security: Security,
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct Security: u32 {
        const NONE         = 0b0000_0000;
        const WEP          = 0b0000_0001;
        const WPA          = 0b0000_0010;
        const WPA2         = 0b0000_0100;
        const ENTERPRISE   = 0b0000_1000;
    }
}

#[derive(Debug)]
pub enum AccessPointCredentials {
    None,
    Wep {
        passphrase: String,
    },
    Wpa {
        passphrase: String,
    },
    Enterprise {
        identity: String,
        passphrase: String,
    },
}

bitflags! {
    pub struct NM80211ApFlags: u32 {
        // access point has no special capabilities
        const AP_FLAGS_NONE                  = 0x0000_0000;
        // access point requires authentication and encryption (usually means WEP)
        const AP_FLAGS_PRIVACY               = 0x0000_0001;
        // access point supports some WPS method
        const AP_FLAGS_WPS                   = 0x0000_0002;
        // access point supports push-button WPS
        const AP_FLAGS_WPS_PBC               = 0x0000_0004;
        // access point supports PIN-based WPS
        const AP_FLAGS_WPS_PIN               = 0x0000_0008;
    }
}

bitflags! {
    pub struct NM80211ApSecurityFlags: u32 {
         // the access point has no special security requirements
        const AP_SEC_NONE                    = 0x0000_0000;
        // 40/64-bit WEP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_WEP40              = 0x0000_0001;
        // 104/128-bit WEP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_WEP104             = 0x0000_0002;
        // TKIP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_TKIP               = 0x0000_0004;
        // AES/CCMP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_CCMP               = 0x0000_0008;
        // 40/64-bit WEP is supported for group/broadcast encryption
        const AP_SEC_GROUP_WEP40             = 0x0000_0010;
        // 104/128-bit WEP is supported for group/broadcast encryption
        const AP_SEC_GROUP_WEP104            = 0x0000_0020;
        // TKIP is supported for group/broadcast encryption
        const AP_SEC_GROUP_TKIP              = 0x0000_0040;
        // AES/CCMP is supported for group/broadcast encryption
        const AP_SEC_GROUP_CCMP              = 0x0000_0080;
        // WPA/RSN Pre-Shared Key encryption is supported
        const AP_SEC_KEY_MGMT_PSK            = 0x0000_0100;
        // 802.1x authentication and key management is supported
        const AP_SEC_KEY_MGMT_802_1X         = 0x0000_0200;
    }
}

impl VariantTo<NM80211ApFlags> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<NM80211ApFlags> {
        value
            .0
            .as_i64()
            .and_then(|v| NM80211ApFlags::from_bits(v as u32))
    }
}

impl VariantTo<NM80211ApSecurityFlags> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<NM80211ApSecurityFlags> {
        value
            .0
            .as_i64()
            .and_then(|v| NM80211ApSecurityFlags::from_bits(v as u32))
    }
}

pub fn request_access_point_scan(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<(), NetworkManagerError> {
    let options: VariantMap = HashMap::new();
    dbus_manager.dbus.call_with_args(
        path,
        NM_WIRELESS_INTERFACE,
        "RequestScan",
        &[&options as &dyn RefArg],
    )?;

    Ok(())
}

pub fn get_device_access_points(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<Vec<String>, NetworkManagerError> {
    dbus_manager
        .dbus
        .property(path, NM_WIRELESS_INTERFACE, "AccessPoints")
}

fn get_access_point_ssid(dbus_manager: &NetworkManager, path: &str) -> Option<Ssid> {
    if let Ok(ssid_vec) =
        dbus_manager
            .dbus
            .property::<Vec<u8>>(path, NM_ACCESS_POINT_INTERFACE, "Ssid")
    {
        Some(Ssid::from_bytes(ssid_vec))
    } else {
        None
    }
}

fn get_access_point_strength(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<u32, NetworkManagerError> {
    dbus_manager
        .dbus
        .property(path, NM_ACCESS_POINT_INTERFACE, "Strength")
}

fn get_access_point_flags(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<NM80211ApFlags, NetworkManagerError> {
    dbus_manager
        .dbus
        .property(path, NM_ACCESS_POINT_INTERFACE, "Flags")
}

fn get_access_point_wpa_flags(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<NM80211ApSecurityFlags, NetworkManagerError> {
    dbus_manager
        .dbus
        .property(path, NM_ACCESS_POINT_INTERFACE, "WpaFlags")
}

fn get_access_point_rsn_flags(
    dbus_manager: &NetworkManager,
    path: &str,
) -> Result<NM80211ApSecurityFlags, NetworkManagerError> {
    dbus_manager
        .dbus
        .property(path, NM_ACCESS_POINT_INTERFACE, "RsnFlags")
}

pub fn get_access_point_security(
    manager: &NetworkManager,
    path: &str,
) -> Result<Security, NetworkManagerError> {
    let flags = get_access_point_flags(manager, path)?;
    let wpa_flags = get_access_point_wpa_flags(manager, path)?;
    let rsn_flags = get_access_point_rsn_flags(manager, path)?;

    let mut security = Security::NONE;

    if flags.contains(NM80211ApFlags::AP_FLAGS_PRIVACY)
        && wpa_flags == NM80211ApSecurityFlags::AP_SEC_NONE
        && rsn_flags == NM80211ApSecurityFlags::AP_SEC_NONE
    {
        security |= Security::WEP;
    }

    if wpa_flags != NM80211ApSecurityFlags::AP_SEC_NONE {
        security |= Security::WPA;
    }

    if rsn_flags != NM80211ApSecurityFlags::AP_SEC_NONE {
        security |= Security::WPA2;
    }

    if wpa_flags.contains(NM80211ApSecurityFlags::AP_SEC_KEY_MGMT_802_1X)
        || rsn_flags.contains(NM80211ApSecurityFlags::AP_SEC_KEY_MGMT_802_1X)
    {
        security |= Security::ENTERPRISE;
    }

    Ok(security)
}

pub fn get_access_point(
    manager: &NetworkManager,
    path: &str,
) -> Result<Option<AccessPoint>, NetworkManagerError> {
    if let Some(ssid) = get_access_point_ssid(manager, path) {
        let strength = get_access_point_strength(manager, path)?;
        let security = get_access_point_security(manager, path)?;
        let access_point = AccessPoint {
            path: path.to_string(),
            ssid: ssid,
            strength: strength,
            security: security,
        };

        Ok(Some(access_point))
    } else {
        Ok(None)
    }
}

pub fn security_string(access_point: &AccessPoint) -> &str {
    if access_point.security.contains(Security::ENTERPRISE) {
        "enterprise"
    } else if access_point.security.contains(Security::WPA2)
        || access_point.security.contains(Security::WPA)
    {
        "wpa"
    } else if access_point.security.contains(Security::WEP) {
        "wep"
    } else {
        "none"
    }
}
