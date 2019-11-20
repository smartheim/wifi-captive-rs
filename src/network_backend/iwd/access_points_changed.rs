//! # Access points change stream
//! Provides a stream of [`WifiConnectionEvent`]s.

use dbus::message::SignalArgs;
use futures_util::future::BoxFuture;
use futures_util::stream::Stream;
use futures_util::FutureExt;
use std::pin::Pin;
use std::task;
use std::task::Poll;

use crate::dbus_tokio::SignalStream;
use crate::network_backend::{NetworkBackend, NM_BUSNAME};
use crate::network_interface::{WifiConnection, WifiConnectionEvent, WifiConnectionEventType};
use crate::utils::take_optional;
use crate::CaptivePortalError;

use super::generated::iwd;
use std::collections::HashSet;

struct AccessPointChanged {
    pub path: String,
    pub interfaces_and_properties: HashSet<String>,
    pub event: WifiConnectionEventType,
}

type APAddedType =
    SignalStream<iwd::OrgFreedesktopDBusObjectManagerInterfacesAdded, AccessPointChanged>;
type APRemovedType =
    SignalStream<iwd::OrgFreedesktopDBusObjectManagerInterfacesRemoved, AccessPointChanged>;
type InnerFutureType = Result<WifiConnection, CaptivePortalError>;

pub struct AccessPointsChangedStream<'a> {
    inner_stream_added: APAddedType,
    inner_stream_removed: APRemovedType,
    inner_future: Option<BoxFuture<'a, InnerFutureType>>,
    inner_future_event_type: WifiConnectionEventType,
    network_manager: &'a NetworkBackend,
}

impl<'a> AccessPointsChangedStream<'a> {
    pub async fn new(
        network_manager: &'a NetworkBackend,
    ) -> Result<AccessPointsChangedStream<'a>, CaptivePortalError> {
        // This is implemented via stream merging, because each subscription is encapsulated in its own stream.

        let rule_added = iwd::OrgFreedesktopDBusObjectManagerInterfacesAdded::match_rule(
            Some(&NM_BUSNAME.to_owned().into()),
            Some(&"/".into()),
        )
        .static_clone();

        let rule_removed = iwd::OrgFreedesktopDBusObjectManagerInterfacesRemoved::match_rule(
            Some(&NM_BUSNAME.to_owned().into()),
            Some(&"/".into()),
        )
        .static_clone();

        let inner_stream_added: APAddedType = SignalStream::new(
            network_manager.conn.clone(),
            rule_added,
            Box::new(
                |v: iwd::OrgFreedesktopDBusObjectManagerInterfacesAdded| AccessPointChanged {
                    event: WifiConnectionEventType::Added,
                    path: v.object_path.to_string(),
                    interfaces_and_properties: v
                        .interfaces_and_properties
                        .into_iter()
                        .map(|f| f.0)
                        .collect(),
                },
            ),
        )
        .await?;

        let inner_stream_removed: APRemovedType = SignalStream::new(
            network_manager.conn.clone(),
            rule_removed,
            Box::new(|v: iwd::OrgFreedesktopDBusObjectManagerInterfacesRemoved| {
                AccessPointChanged {
                    event: WifiConnectionEventType::Removed,
                    path: v.object_path.to_string(),
                    interfaces_and_properties: v.interfaces.into_iter().collect(),
                }
            }),
        )
        .await?;

        Ok(Self {
            inner_stream_added,
            inner_stream_removed,
            network_manager,
            inner_future: None,
            inner_future_event_type: WifiConnectionEventType::Added,
        })
    }
}

impl<'a> Stream for AccessPointsChangedStream<'a> {
    type Item = Result<WifiConnectionEvent, CaptivePortalError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut task::Context,
    ) -> task::Poll<Option<Self::Item>> {
        // This stream merges the Add/Remove streams of the dbus API. But we do not just want to return
        // the changed network manager dbus path, but an actual "WifiConnectionEvent". We need to call
        // a network_manager async method "access_point" for this.
        //
        // If such a future is to-be-resolved: Do this first.

        if let Some(ref mut inner) = self.inner_future {
            match inner.as_mut().poll(ctx) {
                Poll::Ready(ap) => {
                    let inner = Poll::Ready(Some(ap.map(|ap| WifiConnectionEvent {
                        connection: ap,
                        event: self.inner_future_event_type,
                    })));
                    take_optional(self.as_mut(), |me| &mut me.inner_future);
                    return inner;
                },
                Poll::Pending => return Poll::Pending,
            }
        }

        let inner_stream_added = unsafe {
            self.as_mut()
                .map_unchecked_mut(|me| &mut me.inner_stream_added)
        };
        match inner_stream_added.poll_next(ctx) {
            Poll::Ready(Some((access_point_changed, _path))) => {
                let access_point_changed: AccessPointChanged = access_point_changed;
                if access_point_changed
                    .interfaces_and_properties
                    .contains("net.connman.iwd.Network")
                {
                    self.inner_future_event_type = access_point_changed.event;
                    self.inner_future = Some(
                        self.network_manager
                            .access_point(access_point_changed.path)
                            .boxed(),
                    );
                    return self.poll_next(ctx);
                }
            },
            _ => {},
        }

        let inner_stream_removed = unsafe {
            self.as_mut()
                .map_unchecked_mut(|me| &mut me.inner_stream_removed)
        };
        match inner_stream_removed.poll_next(ctx) {
            Poll::Ready(Some((access_point_changed, _path))) => {
                let access_point_changed: AccessPointChanged = access_point_changed;
                if access_point_changed
                    .interfaces_and_properties
                    .contains("net.connman.iwd.Network")
                {
                    self.inner_future_event_type = access_point_changed.event;
                    self.inner_future = Some(
                        self.network_manager
                            .access_point(access_point_changed.path)
                            .boxed(),
                    );
                    return self.poll_next(ctx);
                }
            },
            _ => {},
        }

        task::Poll::Pending
    }
}
