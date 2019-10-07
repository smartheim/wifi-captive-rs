use super::config::Config;

use crate::http_server::sse;
use crate::nm::{AccessPointCredentials, NetworkManager, NetworkManagerEvent, WifiConnection, WifiConnectionEvent, SSID, NetworkManagerState};
use crate::{dhcp_server, dns_server, http_server, CaptivePortalError};
use futures_util::future::Either;
use futures_util::try_future::try_select;
use log::info;
use pin_utils::pin_mut;
use std::net::SocketAddrV4;
use std::time::Duration;

/// The programs state machine. Each state carries its required data, no side-effects.
/// The configuration and network manager connection are moved between states.
///
/// All states transition into StartUp if the dbus connection to the network manager got lost.
pub enum StateMachine {
    /// Starts a dbus connection to the system bus.
    /// Connects to network manager, starts the service if necessary.
    ///
    /// # Transitions:
    /// **Connected** -> If network manager reports active connections and a "connected" state.
    /// **TryReconnect** -> If no connection is active
    ///
    /// # Errors:
    /// Error out if network manager cannot be reached.
    StartUp(Config),

    /// Scans for access points and tries to connect to already known ones.
    ///
    /// # Transitions:
    /// **Connected** -> If network manager transitioned into a connected state.
    /// **ActivatePortal** -> If no connection can be established
    ///
    /// # Errors:
    /// Fails if network manager permissions do not allow to issue wifi scans or connect to
    /// access points. Error out if network manager cannot be reached.
    TryReconnect(Config, NetworkManager),

    /// The device is connected, as reported by network manager
    ///
    /// # Events:
    /// Listens to network manager for connection state changes
    ///
    /// # Transitions:
    /// **ActivatePortal** -> On connection lost after a grace period.
    Connected(Config, NetworkManager),

    /// Activates a wifi hotspot and portal page.
    /// Starts up an http server, a dns server and a dhcp server.
    ///
    /// **IF** network manager reported connections:
    /// Starts a timer to periodically (5 min) check if a connection to an already configured wifi
    /// can be re-established. The portal must be disabled for a few seconds to perform the wifi scan.
    ///
    /// # Transitions:
    /// **Connect** -> When the user requests to connect to a wifi access point via the http server.
    /// **Connected** -> When a connection could be established
    ActivatePortal(Config, NetworkManager),

    /// Tries to connect to the given access point.
    ///
    /// # Transitions:
    /// **Connected** First stores the ssid+passphrase+identity in Config then transition in the connected state.
    /// **ActivatePortal** If the connection fails after a few attempts
    Connect(Config, NetworkManager, SSID, AccessPointCredentials),

    /// Quits the program
    ///
    /// Shuts down the network manager connection.
    Exit(NetworkManager),
}

impl StateMachine {
    pub async fn progress(self) -> Result<Option<StateMachine>, CaptivePortalError> {
        match self {
            StateMachine::StartUp(config) => {
                info!("Starting up");
                let nm = NetworkManager::new(&config).await?;

                let state = nm.state().await?;
                Ok(match state {
                    NetworkManagerState::Unknown | NetworkManagerState::Asleep | NetworkManagerState::Disconnected => {
                        Some(StateMachine::ActivatePortal(config, nm))
                    }
                    NetworkManagerState::Disconnecting | NetworkManagerState::Connecting => {
                        tokio_timer::sleep(Duration::from_millis(500)).await;
                        Some(StateMachine::TryReconnect(config, nm))
                    }
                    NetworkManagerState::ConnectedLocal | NetworkManagerState::ConnectedSite | NetworkManagerState::ConnectedGlobal => {
                        Some(StateMachine::TryReconnect(config, nm))
                    }
                })
            }
            StateMachine::TryReconnect(config, nm) => {
                Ok(Some(StateMachine::ActivatePortal(config, nm)))
            }
            StateMachine::Connected(config, nm) => {
                info!("Connected");
                Ok(Some(StateMachine::Exit(nm)))
            }
            StateMachine::ActivatePortal(config, nm) => {
                info!("Activating portal");

                let (http_server, http_exit) = http_server::HttpServer::new(SocketAddrV4::new(
                    config.gateway.clone(),
                    config.listening_port,
                ));

                let http_state = http_server.state.clone();
                {
                    let list = nm.list_access_points().await?;
                    let mut state = http_state.lock().unwrap();
                    state.connections.0.extend(list);
                }

                tokio::spawn(async move {
                    loop {
                        tokio_timer::sleep(Duration::from_secs(2)).await;

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
                    tokio_timer::sleep(Duration::from_secs(5500)).await;
                    Ok(())
                };
                pin_mut!(timed_future);
                let either = try_select(http_server_future, timed_future)
                    .await
                    .map_err(|e| e.factor_first().0)?;
                if let Either::Left((Some(connect_data), _)) = either {
                    info!("Almost done 2: {:?}", connect_data);
                }

                let _ = http_exit.send(());
                let _ = dns_exit.send(());
                let _ = dhcp_exit.send(());

                Ok(Some(StateMachine::Exit(nm)))
            }
            StateMachine::Connect(config, nm, network, credentials) => {
                info!("Connecting ...");
                Ok(Some(StateMachine::Exit(nm)))
            }
            StateMachine::Exit(nm) => {
                info!("Exiting");
                nm.quit();
                Ok(None)
            }
        }
    }
}
