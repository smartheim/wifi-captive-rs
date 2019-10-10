use crate::{CaptivePortalError, http_server, dns_server, dhcp_server};
use crate::nm::{WifiConnectionEvent, NetworkManager};
use crate::http_server::WifiConnectionRequest;

use std::net::SocketAddrV4;
use std::time::Duration;
use pin_utils::pin_mut;
use std::path::PathBuf;
use std::str::FromStr;
use futures_util::StreamExt;
use crate::utils::try_timed_future;

pub async fn start_portal(nm: &NetworkManager, config: &crate::config::Config) -> Result<Option<WifiConnectionRequest>, CaptivePortalError> {
    let (http_server, http_exit) = http_server::HttpServer::new(SocketAddrV4::new(
        config.gateway.clone(),
        config.listening_port,
    ), nm.clone(), config.ui_directory.clone().unwrap_or(PathBuf::from_str(env!("CARGO_MANIFEST_DIR")).unwrap()));

    let http_state = http_server.state.clone();
    {
        let list = nm.list_access_points().await?;
        let mut state = http_state.lock().unwrap();
        state.connections.0.extend(list);
    };

    // Watch access point changes and inform the http server. It will issue a server-send-event
    // to connected browsers.
    let (list_changes_exit_handler, list_changes_exit_receiver) = tokio::sync::oneshot::channel::<()>();
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
                let event = WifiConnectionEvent {
                    connection: ap,
                    event: access_point_changed.event,
                };
                http_server::update_network(http_state.clone(), event).await;
            }
        };
        pin_mut!(changes_future);

        let exit_receiver = async move {
            let r = list_changes_exit_receiver.await;
            if let Err(e) = r {
                warn!("Error in list_changes_exit_receiver: {}", e);
            }
            Ok::<(), CaptivePortalError>(())
        };
        pin_mut!(exit_receiver);
        let _ = futures_util::future::select(changes_future, exit_receiver).await;
    });

    let (mut dns_server, dns_exit) = dns_server::CaptiveDnsServer::new(
        SocketAddrV4::new(config.gateway.clone(), config.dns_port),
    );
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

    // Wait for any of the given futures (timeout, server) to finish.
    // If the timer finished, call all exit handlers to make the rest of the futures finish

    let http_server_future = http_server.run();

    let r = try_timed_future(http_server_future, Duration::from_secs(350)).await?;
    let result = if let Some(connect_data) = r {
        connect_data
    } else {
        None
    };

    let _ = list_changes_exit_handler.send(());
    let _ = http_exit.send(());
    let _ = dns_exit.send(());
    let _ = dhcp_exit.send(());

    Ok(result)
}