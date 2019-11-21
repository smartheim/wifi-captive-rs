//! # This module contains the portal type.

use super::http_server::WifiConnectionRequest;
use super::network_backend::{AccessPointsChangedStream, NetworkBackend};
use super::network_interface::WifiConnection;
use super::utils::take_optional;
use super::{dhcp_server, dns_server, http_server, CaptivePortalError};

use futures_core::future::BoxFuture;
use futures_util::{FutureExt, StreamExt};
use std::future::Future;
use std::net::SocketAddrV4;
use std::pin::Pin;
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
    hotspot_stopped_fut: Option<BoxFuture<'a, Result<(), CaptivePortalError>>>,
    /// The http server future. Will be polled by this wrapping future.
    http_server: Pin<
        Box<dyn Future<Output = Result<Option<WifiConnectionRequest>, CaptivePortalError>> + Send>,
    >,
}

impl<'a> Portal<'a> {
    /// The configuration should contain a ui_directory, if the UI is not embedded. If that is not set,
    /// the environment variable CARGO_MANIFEST_DIR will be used, which is only useful during development.
    pub fn new(
        nm: &'a NetworkBackend,
        config: &crate::config::Config,
        wifi_sta_active_connection: dbus::Path<'static>,
        wifi_access_points: Vec<WifiConnection>,
        timeout: Duration,
    ) -> Result<(Portal<'a>, tokio::sync::oneshot::Sender<()>), CaptivePortalError> {
        let (http_server, http_exit) = http_server::HttpServer::new(
            SocketAddrV4::new(config.gateway.clone(), config.listening_port),
            nm.clone(),
            config.get_ui_directory(),
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
        tokio::spawn(async move {
            let stream = AccessPointsChangedStream::new(&nm_clone).await;
            let mut stream = match stream {
                Err(e) => {
                    error!("{}", e);
                    return;
                },
                Ok(stream) => stream,
            };
            for event in stream.next().await {
                match event {
                    Err(e) => {
                        error!("{}", e);
                        return;
                    },
                    Ok(event) => http_server::update_network(http_state.clone(), event).await,
                }
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
            hotspot_stopped_fut: Some(nm.on_hotspot_stopped(wifi_sta_active_connection).boxed()),
        };

        Ok((portal, exit_handler))
    }
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

        if let Some(connection_state_change_fut) = self.hotspot_stopped_fut.as_mut() {
            if let Poll::Ready(_) = connection_state_change_fut.as_mut().poll(cx) {
                exit_soon = true;
                take_optional(self.as_mut(), |me| &mut me.hotspot_stopped_fut);
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
