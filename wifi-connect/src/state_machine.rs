use super::config::Config;

use log::info;
use futures_util::try_future::{try_select, try_join};
use futures_util::future::select;
use futures::future;
use pin_utils::pin_mut;
use crate::{errors, http_server, CaptivePortalError};
use std::time::Duration;
use crate::http_server::WifiConnection;
use futures_util::future::Either;

/// The programs state machine. Each state carries its required data, no side-effects.
/// The configuration is moved between states.
///
/// All states transition into StartUp if the dbus connection to the network manager got lost.
pub enum StateMachine {
    /// Starts a dbus connection to the system bus.
    /// Connects to network manager, starts the service if necessary.
    ///
    /// Transitions:
    /// **Exit** -> Quits if network manager cannot be reached
    /// **Connected** -> If a connection exists and works
    /// **Connect** -> If a wifi connection is configured in the given Config
    ///    but no connection has been established yet.
    /// **ActivatePortal** -> If no connection is active and nothing has been configured in Config.
    StartUp(Config),
    /// Listens to network manager for connection changes
    ///
    /// Transitions:
    /// **ActivatePortal** -> On connection lost after a grace period.
    Connected(Config),
    /// Listens to network manager for connection changes.
    ///
    /// **IF** config has a ssid(+password+identity) set:
    /// Starts a timer to periodically (5 min) check if a connection to that already configured wifi
    /// can be re-established. The portal must be disabled for that moment.
    ///
    /// Activates a wifi hotspot and portal page.
    /// Starts up an http server, a dns server and a dhcp server.
    ///
    /// Transitions:
    /// **Connect** -> When the user requests to connect to a wifi access point via the http server.
    /// **Connected** -> When a connection could be established
    ActivatePortal(Config),
    /// Tries to connect to the given access point.
    ///
    /// Transitions:
    /// **Connected** First stores the ssid+passphrase+identity in Config then transition in the connected state.
    /// **ActivatePortal** If the connection fails after a few attempts
    Connect {
        config: Config,
        ssid: String,
        identity: Option<String>,
        passphrase: Option<String>,
    },
    /// Quits the program
    Exit,
}

//TODO later called delay_for: tokio_timer::delay_for(Duration::from_secs(5))
async fn timed(tx: tokio::sync::oneshot::Sender<()>) -> Result<Option<http_server::WifiConnectionRequest>, CaptivePortalError> {
    tokio_timer::sleep(Duration::from_secs(5500)).await;
    tx.send(())
        .map_err(|_| CaptivePortalError::Generic("Failed to message pass"))?;
    Ok(None)
}

impl StateMachine {
    pub async fn progress(&self) -> Result<StateMachine, CaptivePortalError> {
        match self {
            StateMachine::StartUp(config) => {
                info!("Starting up");

                let (tx, http_server) = http_server::HttpServer::new(config.gateway.clone(), config.listening_port);

                {
                    let mut state = http_server.state.lock().unwrap();
                    state.connections.0.push(WifiConnection {
                        ssid: "My SSID".to_string(),
                        uuid: "".to_string(),
                        security: "".to_string(),
                        strength: 66,
                    });
                }

                let f1 = http_server.run();
                pin_mut!(f1);
                let f2 = timed(tx);
                pin_mut!(f2);
                let either = try_select(f1, f2).await
                    .map_err(|e| e.factor_first().0)?;
                if let Either::Left((Some(connect_data), _)) = either {
                    info!("Almost done 2: {:?}", connect_data);
                }

                Ok(StateMachine::Exit)
            }
            StateMachine::Connected(_) => {
                info!("Connected");
                Ok(StateMachine::Exit)
            }
            StateMachine::ActivatePortal(config) => {
                info!("Activating portal");
                let mut dhcp_server = crate::dhcp_server::Server::new(config.gateway.clone())?;
                try_join(dhcp_server.run(), future::ready(Ok::<i32, std::io::Error>(1))).await?;

                Ok(StateMachine::Exit)
            }
            StateMachine::Connect { .. } => {
                info!("Connecting ...");
                Ok(StateMachine::Exit)
            }
            StateMachine::Exit => {
                info!("Exiting");
                Ok(StateMachine::Exit)
            }
        }
    }
}
