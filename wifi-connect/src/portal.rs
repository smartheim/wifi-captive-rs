//! # This module contains the portal type.

use super::http_server::{HttpServerStateSync, WifiConnectionRequest};
use super::nm::{
    on_active_connection_state_change, AccessPointChangeReturnType, ConnectionState,
    NetworkManager, WifiConnection, WifiConnectionEvent,
};
use super::{dhcp_server, dns_server, http_server, CaptivePortalError};

use futures_core::future::BoxFuture;
use futures_util::{FutureExt, StreamExt};
use std::future::Future;
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::task;
use std::task::Poll;
use std::time::Duration;
use tokio::timer::Delay;

/// The portal type offers a web-ui and redirection services ("Captive Portal"). It stays online
/// for a certain configurable time and returns when the user has selected a wifi SSID and entered
/// credentials.
///
/// # Implementation details
/// The portal spawns several background tasks for dns, dhcp, access point changes.
/// It is itself a future that polls the timeout, connection-changed and webserver inner futures.
/// It also resolves when the user has selected a wifi connection from the UI.
pub struct Portal<'a> {
    /// Used to quit the server by the timeout or user wifi selection
    http_exit: Option<tokio::sync::oneshot::Sender<()>>,
    /// As soon as Portal is dropped, the dns server will stop
    #[allow(dead_code)]
    dns_exit: tokio::sync::oneshot::Sender<()>,
    /// As soon as Portal is dropped, the dhcp server will stop
    #[allow(dead_code)]
    dhcp_exit: tokio::sync::oneshot::Sender<()>,
    /// Internal: This future is polled by this wrapping future to determine if outside wants us to quit.
    exit_receiver: Option<tokio::sync::oneshot::Receiver<()>>,
    /// The timeout future. Will be polled by this wrapping future.
    timeout: Option<Delay>,
    /// The connection changed future. Will be polled by this wrapping future.
    connection_state_change_fut: Option<BoxFuture<'a, Result<ConnectionState, CaptivePortalError>>>,
    /// The http server future. Will be polled by this wrapping future.
    http_server: Pin<
        Box<dyn Future<Output = Result<Option<WifiConnectionRequest>, CaptivePortalError>> + Send>,
    >,
}

impl<'a> Portal<'a> {
    /// The configuration should contain a ui_directory, if the UI is not embedded. If that is not set,
    /// the environment variable CARGO_MANIFEST_DIR will be used, which is only useful during development.
    pub fn new(
        nm: &'a NetworkManager,
        config: &crate::config::Config,
        wifi_sta_active_connection: dbus::Path<'static>,
        wifi_access_points: Vec<WifiConnection>,
        timeout: Duration,
    ) -> Result<(Portal<'a>, tokio::sync::oneshot::Sender<()>), CaptivePortalError> {
        let ui_directory = config
            .ui_directory
            .clone()
            .unwrap_or(PathBuf::from_str(env!("CARGO_MANIFEST_DIR")).unwrap_or("".into()));

        let (http_server, http_exit) = http_server::HttpServer::new(
            SocketAddrV4::new(config.gateway.clone(), config.listening_port),
            nm.clone(),
            ui_directory,
        );

        let mut state = http_server
            .state
            .lock()
            .expect("Lock http_state mutex for portal");
        state.connections.0.extend(wifi_access_points);
        drop(state);

        let http_state = http_server.state.clone();

        let (mut dns_server, dns_exit) = dns_server::CaptiveDnsServer::new(SocketAddrV4::new(
            config.gateway.clone(),
            config.dns_port,
        ));
        let (mut dhcp_server, dhcp_exit) = dhcp_server::DHCPServer::new(SocketAddrV4::new(
            config.gateway.clone(),
            config.dhcp_port,
        ));

        tokio::spawn(async move {
            if let Err(e) = dns_server.run().await {
                error!("{}", e);
            }
        });
        tokio::spawn(async move {
            if let Err(e) = dhcp_server.run().await {
                error!("{}", e);
            }
        });

        let nm_clone = nm.clone();
        let state_clone = http_state.clone();
        tokio::spawn(async move {
            let c = nm_clone.on_access_point_list_changes().await;
            match c {
                Err(e) => {
                    error!("{}", e);
                },
                Ok(mut c) => {
                    while let Ok(_) =
                        next_access_point_changed(&nm_clone, &mut c, state_clone.clone()).await
                    {
                    }
                },
            }
        });

        let (exit_handler, exit_receiver) = tokio::sync::oneshot::channel::<()>();

        let portal = Portal {
            http_server: Box::pin(http_server.run()),
            dns_exit,
            dhcp_exit,
            exit_receiver: Some(exit_receiver),
            http_exit: Some(http_exit),
            timeout: Some(tokio::timer::delay_for(timeout)),
            connection_state_change_fut: Some(
                on_active_connection_state_change(nm, wifi_sta_active_connection).boxed(),
            ),
        };

        Ok((portal, exit_handler))
    }
}

/// Return a future that resolves on the next access point that got discovered / disappeared.
async fn next_access_point_changed(
    nm: &NetworkManager,
    access_point_changes_fut: &mut AccessPointChangeReturnType,
    http_state: HttpServerStateSync,
) -> Result<(), CaptivePortalError> {
    let (access_point_changed, _path) = match access_point_changes_fut.next().await {
        Some(next) => next,
        None => return Ok(()),
    };

    let ap = nm.access_point(access_point_changed.path).await?;
    if let Some(ap) = ap {
        let event = WifiConnectionEvent {
            connection: ap,
            event: access_point_changed.event,
        };
        http_server::update_network(http_state.clone(), event).await;
    }
    Ok(())
}

/// Takes an optional field member of the portal and sets the optional to None.
///
/// Safety: Because the optional fields are never moved, this is considered safe, albeit the pinning.
fn take_optional<'a, F, X>(mut portal: Pin<&mut Portal<'a>>, fun: F)
where
    F: for<'r> FnOnce(&'r mut Portal<'a>) -> &'r mut Option<X>,
    X: Unpin + 'a,
{
    // Safety: we never move `self.value` (the Optional)
    let exit_receiver = unsafe { portal.as_mut().map_unchecked_mut(fun) };
    // Remove future out of optional
    let _ = exit_receiver.get_mut().take();
}

/// The portal is also a future. It polls on various exit conditions like the timeout,
/// a user selected wifi, or when the active connection changes its state. And it
/// also polls on the webserver of course.
///
/// All polled futures are wrapped in Optional in the portal structure, because we do not
/// want to call a resolved future again.
impl<'a> Future for Portal<'a> {
    type Output = Result<Option<WifiConnectionRequest>, CaptivePortalError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        let mut exit_soon = false;

        // First check if we got cancelled from outside
        if let Some(exit_receiver) = self.exit_receiver.as_mut() {
            if let Poll::Ready(_) = exit_receiver.poll_unpin(cx) {
                exit_soon = true;
                take_optional(self.as_mut(), |me| &mut me.exit_receiver);
            }
        }

        if let Some(connection_state_change_fut) = self.connection_state_change_fut.as_mut() {
            if let Poll::Ready(_) = connection_state_change_fut.as_mut().poll(cx) {
                exit_soon = true;
                take_optional(self.as_mut(), |me| &mut me.connection_state_change_fut);
            }
        }

        if let Some(timeout) = self.timeout.as_mut() {
            if let Poll::Ready(_) = timeout.poll_unpin(cx) {
                exit_soon = true;
                take_optional(self.as_mut(), |me| &mut me.timeout);
            }
        }

        if exit_soon && self.http_exit.is_some() {
            take_optional(self.as_mut(), |me| &mut me.http_exit);
        }

        // Safety: we never move `self.value`
        let http_server = unsafe { self.as_mut().map_unchecked_mut(|me| &mut me.http_server) };
        if let Poll::Ready(v) = http_server.poll(cx) {
            return Poll::Ready(v);
        }

        Poll::Pending
    }
}
