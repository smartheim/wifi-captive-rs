//! Security related types and bit fields are declared in this module.

use super::NM_BUSNAME;
use crate::CaptivePortalError;
use bitflags::bitflags;
use dbus::nonblock;
use dbus::nonblock::SyncConnection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone)]
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
    #[derive(Serialize, Deserialize)]
    pub struct Security: u32 {
        const NONE         = 0b0000_0000;
        const WEP          = 0b0000_0001;
        const WPA          = 0b0000_0010;
        const WPA2         = 0b0000_0100;
        const ENTERPRISE   = 0b0000_1000;
    }
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

impl Security {
    pub fn as_str(self) -> &'static str {
        if self.contains(Security::ENTERPRISE) {
            "enterprise"
        } else if self.contains(Security::WPA2) || self.contains(Security::WPA) {
            "wpa"
        } else if self.contains(Security::WEP) {
            "wep"
        } else {
            "none"
        }
    }
}

// Converts a set of credentials into the [`AccessPointCredentials`] type.
pub fn credentials_from_data(
    passphrase: Option<String>,
    identity: Option<String>,
    mode: &str,
) -> Result<AccessPointCredentials, CaptivePortalError> {
    match mode {
        "enterprise" => Ok(AccessPointCredentials::Enterprise {
            identity: identity.ok_or(CaptivePortalError::no_shared_key())?,
            passphrase: passphrase.ok_or(CaptivePortalError::no_shared_key())?,
        }),
        "wpa" => Ok(AccessPointCredentials::Wpa {
            passphrase: passphrase.ok_or(CaptivePortalError::no_shared_key())?,
        }),
        "wep" => Ok(AccessPointCredentials::Wep {
            passphrase: passphrase.ok_or(CaptivePortalError::no_shared_key())?,
        }),
        "open" | "none" | "" => Ok(AccessPointCredentials::None),
        _ => Err(CaptivePortalError::GenericO(format!("Expected an encryption mode. Got: {}", &mode))),
    }
}

// Returns the encryption mode of an dbus access point path. The encryption mode depends on
// quite a few flags and that's why it is encapsulated into its own method.
pub async fn get_access_point_security(
    conn: Arc<SyncConnection>,
    ap_path: &dbus::Path<'_>,
) -> Result<Security, super::CaptivePortalError> {
    let access_point_data = nonblock::Proxy::new(NM_BUSNAME, ap_path, conn.clone());
    use super::access_point::AccessPoint;
    let flags = NM80211ApFlags::from_bits(access_point_data.flags().await?)
        .unwrap_or(NM80211ApFlags::AP_FLAGS_NONE);
    let wpa_flags = NM80211ApSecurityFlags::from_bits(access_point_data.wpa_flags().await?)
        .unwrap_or(NM80211ApSecurityFlags::AP_SEC_NONE);
    let rsn_flags = NM80211ApSecurityFlags::from_bits(access_point_data.rsn_flags().await?)
        .unwrap_or(NM80211ApSecurityFlags::AP_SEC_NONE);

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
