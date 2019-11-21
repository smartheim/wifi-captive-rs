//! # Access points change stream
//! Provides a stream of [`WifiConnectionEvent`]s.

use dbus::message::SignalArgs;
use futures_util::stream::select;
use futures_util::stream::StreamExt;

use super::device;
use crate::dbus_tokio::SignalStream;
use crate::network_backend::nm::generated::device::{DeviceWirelessAccessPointAdded, DeviceWirelessAccessPointRemoved};
use crate::network_backend::{NetworkBackend, NM_BUSNAME};
use crate::network_interface::WifiConnectionEventType;
use crate::CaptivePortalError;
use futures_core::stream::BoxStream;

pub struct AccessPointChanged {
    pub path: String,
    pub event: WifiConnectionEventType,
}

fn helper_1(v: (DeviceWirelessAccessPointAdded, String)) -> AccessPointChanged {
    AccessPointChanged {
        event: WifiConnectionEventType::Added,
        path: v.0.access_point.to_string(),
    }
}

fn helper_2(v: (DeviceWirelessAccessPointRemoved, String)) -> AccessPointChanged {
    AccessPointChanged {
        event: WifiConnectionEventType::Removed,
        path: v.0.access_point.to_string(),
    }
}

pub async fn ap_changed_stream(
    network_manager: &NetworkBackend,
) -> Result<BoxStream<'static, AccessPointChanged>, CaptivePortalError> {
    // This is implemented via stream merging, because each subscription is encapsulated in its own stream.

    let rule_added = device::DeviceWirelessAccessPointAdded::match_rule(
        Some(&NM_BUSNAME.to_owned().into()),
        Some(&network_manager.wifi_device_path.clone().into()),
    )
    .static_clone();

    let rule_removed = device::DeviceWirelessAccessPointRemoved::match_rule(
        Some(&NM_BUSNAME.to_owned().into()),
        Some(&network_manager.wifi_device_path.clone().into()),
    )
    .static_clone();

    let inner_stream_added =
        SignalStream::<device::DeviceWirelessAccessPointAdded>::new(network_manager.conn.clone(), rule_added)
            .await?
            .map(helper_1);

    let inner_stream_removed =
        SignalStream::<device::DeviceWirelessAccessPointRemoved>::new(network_manager.conn.clone(), rule_removed)
            .await?
            .map(helper_2);

    Ok(select(inner_stream_added, inner_stream_removed).boxed())
}
