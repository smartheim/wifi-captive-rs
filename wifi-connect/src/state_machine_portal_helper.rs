use crate::{CaptivePortalError, http_server, dns_server, dhcp_server};
use crate::nm::{NetworkManagerEvent, WifiConnectionEvent, WifiConnection, NetworkManager};
use crate::http_server::WifiConnectionRequest;

use std::net::SocketAddrV4;
use std::time::Duration;
use futures_util::future::Either;
use pin_utils::pin_mut;
use futures_util::try_future::try_select;
use std::path::PathBuf;
use std::str::FromStr;

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

    tokio::spawn(async move {
        loop {
            tokio_timer::delay_for(Duration::from_secs(2)).await;

            use chrono::{Timelike, Utc};
            let now = Utc::now();
            let hour = now.hour();
            let d = format!("{:02}:{:02}:{:02}", hour, now.minute(), now.second(), );
            let _e = WifiConnectionEvent {
                connection: WifiConnection {
                    ssid: d,
                    hw: "some_id".to_owned(),
                    security: "wpa",
                    strength: now.minute() as u8,
                    frequency: 2412,
                },
                event: NetworkManagerEvent::Added,
            };
            // http_server::update_network(http_state.clone(),e).await;
        }
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
    pin_mut!(http_server_future);
    let timed_future = async move {
        tokio_timer::delay_for(Duration::from_secs(350)).await;
        Ok(())
    };
    pin_mut!(timed_future);
    let either = try_select(http_server_future, timed_future)
        .await
        .map_err(|e| e.factor_first().0)?;
    let result = if let Either::Left((connect_data, _)) = either {
        connect_data
    } else {
        None
    };

    let _ = http_exit.send(());
    let _ = dns_exit.send(());
    let _ = dhcp_exit.send(());

    Ok(result)
}