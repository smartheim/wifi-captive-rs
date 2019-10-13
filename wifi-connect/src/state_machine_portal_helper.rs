use crate::http_server::WifiConnectionRequest;
use crate::nm::{NetworkManager, WifiConnectionEvent};
use crate::{dhcp_server, dns_server, http_server, CaptivePortalError};

use futures_util::future::Either;
use futures_util::StreamExt;
use pin_utils::pin_mut;
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use tokio::future::FutureExt;

pub async fn start_portal(
    nm: &NetworkManager,
    config: &crate::config::Config,
    wifi_sta_active_connection: dbus::Path<'_>,
) -> Result<Option<WifiConnectionRequest>, CaptivePortalError> {
    let (http_server, http_exit) = http_server::HttpServer::new(
        SocketAddrV4::new(config.gateway.clone(), config.listening_port),
        nm.clone(),
        config
            .ui_directory
            .clone()
            .unwrap_or(PathBuf::from_str(env!("CARGO_MANIFEST_DIR")).unwrap_or("".into())),
    );

    let http_state = http_server.state.clone();
    {
        let list = nm.list_access_points().await?;
        let mut state = http_state.lock().expect("Lock http_state mutex for portal");
        state.connections.0.extend(list);
    };

    // Watch access point changes and inform the http server. It will issue a server-send-event
    // to connected browsers.
    let (list_changes_exit_handler, list_changes_exit_receiver) =
        tokio::sync::oneshot::channel::<()>();
    let nm_clone = nm.clone();
    tokio::spawn(async move {
        let changes_future = async move {
            let changes_future = nm_clone.on_access_point_list_changes().await;
            if let Err(e) = changes_future {
                warn!("Error in access point watcher: {}", e);
                return Err::<(), CaptivePortalError>(e);
            };
            let changes_future = changes_future.unwrap();
            pin_mut!(changes_future);
            let mut changes_future = changes_future; // Idea IDE workaround
            loop {
                let r = changes_future.next().await;
                let (access_point_changed, _path) = r.unwrap();
                let ap = nm_clone.access_point(access_point_changed.path).await?;
                if let Some(ap) = ap {
                    let event = WifiConnectionEvent {
                        connection: ap,
                        event: access_point_changed.event,
                    };
                    http_server::update_network(http_state.clone(), event).await;
                }
            }
        };
        pin_mut!(changes_future);

        let exit_receiver = async move {
            let r = list_changes_exit_receiver.await;
            if let Err(_) = r {
                info!("Access point watcher finished");
            }
            Ok::<(), CaptivePortalError>(())
        };
        pin_mut!(exit_receiver);
        let _ = futures_util::future::select(changes_future, exit_receiver).await;
    });

    let (mut dns_server, dns_exit) = dns_server::CaptiveDnsServer::new(SocketAddrV4::new(
        config.gateway.clone(),
        config.dns_port,
    ));
    let (mut dhcp_server, dhcp_exit) =
        dhcp_server::DHCPServer::new(SocketAddrV4::new(config.gateway.clone(), config.dhcp_port));

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

    // Wait for any of the given futures (timeout, server) to finish.

    let http_server_future = http_server.run();

    let http_server_timed_future = http_server_future.timeout(Duration::from_secs(config.retry_in));
    pin_mut!(http_server_timed_future);

    // But also end the portal whenever the active connection (hotspot) state changes
    let conn_change_future = nm.on_active_connection_state_change(wifi_sta_active_connection);
    pin_mut!(conn_change_future);

    // Select on the http_server (with timeout) and the connection state change future
    let select_future =
        futures_util::try_future::try_select(http_server_timed_future, conn_change_future)
            .await;

    // Check if this is a timeout
    let result = match select_future {
        Err(e) => {
            match e {
                Either::Left((_elapsed_error, _)) => {
                    drop(list_changes_exit_handler);
                    drop(http_exit);
                    None
                }
                Either::Right((e, timeout_future)) => {
                    drop(list_changes_exit_handler);
                    drop(http_exit);
                    let _ = timeout_future.await;
                    return Err(e);
                }
            }
        }
        Ok(select_future) => {
            match select_future {
                // If the timer finished, call all exit handlers to make the rest of the futures finish
                Either::Left((http_server_timed_future, _conn_change_future)) => {
                    drop(list_changes_exit_handler);
                    drop(http_exit);
                    http_server_timed_future?
                }
                // Connection state changed -> This means the user didn't had a chance to select a target wifi AP yet.
                Either::Right((_connection_state, http_server_timed_future)) => {
                    drop(list_changes_exit_handler);
                    drop(http_exit);
                    let _ = http_server_timed_future.await;
                    None
                }
            }
        }
    };

    // When we return, all exit handlers of this method are going out of scope.
    // This makes all spawned futures to finish.
    // They will not be awaited in this method, but the executor should drive them to an end.

    // To make the static analyser happy, we "use" the exit handlers here.
    drop(dns_exit);
    drop(dhcp_exit);

    Ok(result)
}
