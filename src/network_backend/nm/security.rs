//! Security related types and bit fields are declared in this module.
//!
//! This contains implementation specific bits only.

use super::NM_BUSNAME;
use dbus::nonblock;
use dbus::nonblock::SyncConnection;
use enumflags2::BitFlags;
//use serde::{Deserialize, Serialize};
use crate::Security;
use std::sync::Arc;

#[allow(non_camel_case_types)]
#[derive(BitFlags, Copy, Clone)]
#[repr(u32)]
pub(crate) enum NM80211ApFlags {
    // access point has no special capabilities
    //AP_FLAGS_NONE = 0x0000_0000,
    // access point requires authentication and encryption (usually means WEP)
    AP_FLAGS_PRIVACY = 0x0000_0001,
    // access point supports some WPS method
    AP_FLAGS_WPS = 0x0000_0002,
    // access point supports push-button WPS
    AP_FLAGS_WPS_PBC = 0x0000_0004,
    // access point supports PIN-based WPS
    AP_FLAGS_WPS_PIN = 0x0000_0008,
}

#[allow(non_camel_case_types)]
#[derive(BitFlags, Copy, Clone)]
#[repr(u32)]
pub(crate) enum NM80211ApSecurityFlags {
    // the access point has no special security requirements
    //AP_SEC_NONE = 0x0000_0000,
    // 40/64-bit WEP is supported for pairwise/unicast encryption
    AP_SEC_PAIR_WEP40 = 0x0000_0001,
    // 104/128-bit WEP is supported for pairwise/unicast encryption
    AP_SEC_PAIR_WEP104 = 0x0000_0002,
    // TKIP is supported for pairwise/unicast encryption
    AP_SEC_PAIR_TKIP = 0x0000_0004,
    // AES/CCMP is supported for pairwise/unicast encryption
    AP_SEC_PAIR_CCMP = 0x0000_0008,
    // 40/64-bit WEP is supported for group/broadcast encryption
    AP_SEC_GROUP_WEP40 = 0x0000_0010,
    // 104/128-bit WEP is supported for group/broadcast encryption
    AP_SEC_GROUP_WEP104 = 0x0000_0020,
    // TKIP is supported for group/broadcast encryption
    AP_SEC_GROUP_TKIP = 0x0000_0040,
    // AES/CCMP is supported for group/broadcast encryption
    AP_SEC_GROUP_CCMP = 0x0000_0080,
    // WPA/RSN Pre-Shared Key encryption is supported
    AP_SEC_KEY_MGMT_PSK = 0x0000_0100,
    // 802.1x authentication and key management is supported
    AP_SEC_KEY_MGMT_802_1X = 0x0000_0200,
}

// Returns the strongest supported encryption mode of an dbus access point path. The encryption mode depends on
// quite a few flags and that's why it is encapsulated into its own method.
pub(crate) async fn get_access_point_security(
    conn: Arc<SyncConnection>,
    ap_path: &dbus::Path<'_>,
) -> Result<Security, super::CaptivePortalError> {
    let access_point_data = nonblock::Proxy::new(NM_BUSNAME, ap_path, conn.clone());
    use super::access_point::AccessPoint;
    let flags: BitFlags<NM80211ApFlags> =
        BitFlags::from_bits(access_point_data.flags().await?).unwrap_or(BitFlags::empty());
    let wpa_flags: BitFlags<NM80211ApSecurityFlags> =
        BitFlags::from_bits(access_point_data.wpa_flags().await?).unwrap_or(BitFlags::empty());
    let rsn_flags: BitFlags<NM80211ApSecurityFlags> =
        BitFlags::from_bits(access_point_data.rsn_flags().await?).unwrap_or(BitFlags::empty());

    if wpa_flags.contains(NM80211ApSecurityFlags::AP_SEC_KEY_MGMT_802_1X)
        || rsn_flags.contains(NM80211ApSecurityFlags::AP_SEC_KEY_MGMT_802_1X)
    {
        return Ok(Security::ENTERPRISE);
    }

    if !rsn_flags.is_empty() {
        return Ok(Security::WPA2);
    }

    if !wpa_flags.is_empty() {
        return Ok(Security::WPA);
    }

    if flags.contains(NM80211ApFlags::AP_FLAGS_PRIVACY) && wpa_flags.is_empty() && rsn_flags.is_empty() {
        return Ok(Security::WEP);
    }

    Ok(Security::NONE)
}
