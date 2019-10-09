use super::config::Config;

use crate::http_server::WifiConnectionRequest;
use crate::nm::{NetworkManager, NetworkManagerState};
use crate::CaptivePortalError;
use log::info;
use std::time::Duration;
use tokio_net::signal;
use futures_util::StreamExt;
use futures_util::future::Either;


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
    /// **TryReconnect** -> On connection lost
    /// **Exit** ->  On ctrl+c
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
    /// **Exit** ->  On ctrl+c
    ActivatePortal(Config, NetworkManager),

    /// Tries to connect to the given access point.
    ///
    /// # Transitions:
    /// **Connected** First stores the ssid+passphrase+identity in Config then transition in the connected state.
    /// **ActivatePortal** If the connection fails after a few attempts
    Connect(Config, NetworkManager, WifiConnectionRequest),

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
                        tokio_timer::delay_for(Duration::from_millis(500)).await;
                        Some(StateMachine::TryReconnect(config, nm))
                    }
                    NetworkManagerState::ConnectedLocal | NetworkManagerState::ConnectedSite | NetworkManagerState::ConnectedGlobal => {
                        Some(StateMachine::Connected(config, nm))
                    }
                })
            }
            StateMachine::TryReconnect(config, nm) => {
                Ok(Some(StateMachine::ActivatePortal(config, nm)))
            }
            StateMachine::Connected(config, nm) => {
                info!("Connected");

                let r = ctrl_c_or_future(nm.print_connection_changes()).await?;
                match r {
                    // Ctrl+C
                    None => Ok(Some(StateMachine::Exit(nm))),
                    Some(_) => Ok(Some(StateMachine::TryReconnect(config, nm)))
                }
            }
            StateMachine::ActivatePortal(config, nm) => {
                info!("Activating portal");

                let r = ctrl_c_or_future(super::state_machine_portal_helper::start_portal(&nm, &config)).await?;
                match r {
                    // Ctrl+C
                    None => Ok(Some(StateMachine::Exit(nm))),
                    // Either the user has entered a wifi connection or a timeout happened
                    Some(wifi_connection) => match wifi_connection {
                        Some(wifi_connection) => Ok(Some(StateMachine::Connect(config, nm, wifi_connection))),
                        // A timeout means that we should retry to connect to an existing connection
                        None => Ok(Some(StateMachine::TryReconnect(config, nm)))
                    }
                }
            }
            StateMachine::Connect(config, nm, network) => {
                info!("Connecting ...");

                let state = nm.connect_to(network.ssid, network.hw, crate::nm::credentials_from_data(network.passphrase, network.identity, network.mode)?).await?;
                match state {
                    crate::nm::ConnectionState::Activated => {
                        Ok(Some(StateMachine::Connected(config, nm)))
                    }
                    _ => {
                        Ok(Some(StateMachine::ActivatePortal(config, nm)))
                    }
                }
            }
            StateMachine::Exit(nm) => {
                info!("Exiting");
                nm.quit();
                Ok(None)
            }
        }
    }
}

async fn ctrl_c_or_future<F, R>(connect_future: F) -> Result<Option<R>, CaptivePortalError>
    where F: std::future::Future<Output=Result<R, CaptivePortalError>>,
          R: Sized {
    use futures_util::try_future::try_select;

    let ctrl_c = async move {
        match signal::ctrl_c() {
            Ok(mut v) => {
                v.next().await;
                Ok(())
            }
            Err(e) => Err(CaptivePortalError::Generic("signal::ctrl_c() failed"))
        }
    };
    pin_utils::pin_mut!(ctrl_c);
    pin_utils::pin_mut!(connect_future);

    let r = try_select(connect_future, ctrl_c).await;
    match r {
        Err(e) => {
            if let Either::Left((e, _)) = e {
                return Err(e);
            }
        }
        Ok(v) => {
            if let Either::Left((v, _)) = v {
                return Ok(Some(v));
            }
        }
    }

    Ok(None)
}