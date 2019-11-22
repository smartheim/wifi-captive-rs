//! # The programs state machine. Each state carries its required data, no side-effects.

use crate::config::Config;
use crate::http_server::WifiConnectionRequest;
use crate::network_backend::NetworkBackend;
use crate::network_interface::credentials_from_data;
use crate::utils::ctrl_c_or_future;
use crate::utils::FutureWithSignalCancel;
use crate::{CaptivePortalError, verify_password};
use crate::ConnectionState;
use crate::NetworkManagerState;
use log::info;
use std::convert::TryInto;
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
    /// **Exit** ->  On ctrl+c
    ///
    /// # Errors:
    /// Fails if network manager permissions do not allow to issue wifi scans or connect to
    /// access points. Error out if network manager cannot be reached.
    TryReconnect(Config, NetworkBackend),

    /// The device is connected, as reported by network manager
    ///
    /// # Events:
    /// Listens to network manager for connection state changes
    ///
    /// # Transitions:
    /// **TryReconnect** -> On connection lost
    /// **Exit** ->  On ctrl+c
    Connected(Config, NetworkBackend),

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
    ActivatePortal(Config, NetworkBackend),

    /// Tries to connect to the given access point.
    ///
    /// # Transitions:
    /// **Connected** First stores the ssid+passphrase+identity in Config then transition in the connected state.
    /// **ActivatePortal** If the connection fails after a few attempts
    Connect(Config, NetworkBackend, WifiConnectionRequest),

    /// Quits the program
    ///
    /// Shuts down the network manager connection.
    Exit(NetworkBackend),
}

impl StateMachine {
    pub async fn progress(self) -> Result<Option<StateMachine>, CaptivePortalError> {
        match self {
            StateMachine::StartUp(config) => {
                let nm = NetworkBackend::new(&config.interface).await?;
                nm.enable_networking_and_wifi().await?;

                let state = nm.state().await?;
                info!("Starting up. Network manager reports state {:?}", state);
                Ok(match state {
                    NetworkManagerState::Unknown | NetworkManagerState::Asleep | NetworkManagerState::Disconnected => {
                        Some(StateMachine::ActivatePortal(config, nm))
                    }
                    NetworkManagerState::Disconnecting | NetworkManagerState::Connecting => {
                        Some(StateMachine::TryReconnect(config, nm))
                    }
                    NetworkManagerState::Connected | NetworkManagerState::ConnectedLimited => {
                        Some(StateMachine::Connected(config, nm))
                    }
                })
            }
            StateMachine::TryReconnect(config, nm) => {
                info!("No connection found. Trying to reestablish");
                nm.enable_networking_and_wifi().await?;

                // Try to connect to an existing connection
                let r =
                    ctrl_c_or_future(nm.try_auto_connect(Duration::from_secs(config.wait_before_reconfigure))).await?;
                match r {
                    // Ctrl+C
                    None => return Ok(Some(StateMachine::Exit(nm))),
                    Some(state) => {
                        if state {
                            return Ok(Some(StateMachine::Connected(config, nm)));
                        }
                    }
                }
                return Ok(Some(StateMachine::ActivatePortal(config, nm)));
            }
            StateMachine::Connected(config, nm) => {
                nm.deactivate_hotspots().await?;

                let c_state = nm
                    .wait_for_connectivity(config.internet_connectivity, Duration::from_secs(5))
                    .await;
                info!("Current connectivity: {:?}", c_state);

                match c_state {
                    Ok(_) => {}
                    Err(CaptivePortalError::NotRequiredConnectivity(_)) => {
                        return Ok(Some(StateMachine::TryReconnect(config, nm)));
                    }
                    Err(e) => return Err(e),
                }

                if config.quit_after_connected {
                    return Ok(Some(StateMachine::Exit(nm)));
                }

                // Await a connectivity change, ctrl+c or the timeout
                let r = nm
                    .wait_for_connectivity_lost(config.internet_connectivity, Duration::from_secs(config.retry_in))
                    .ctrl_c()
                    .await;

                match r {
                    // Ctrl+C
                    None => Ok(Some(StateMachine::Exit(nm))),
                    Some(_) => Ok(Some(StateMachine::TryReconnect(config, nm))),
                }
            }
            StateMachine::ActivatePortal(mut config, nm) => {
                nm.enable_networking_and_wifi().await?;
                nm.deactivate_hotspots().await?;

                update_portal_info_via_file(&mut config);

                info!("Acquire wifi access point list. This may take a minute ...");
                let wifi_access_points = nm.list_access_points(Duration::from_secs(7)).await?;

                use tokio::future::FutureExt;

                let r = nm
                    .hotspot_start(config.ssid.clone(), config.passphrase.clone(), Some(config.gateway))
                    .timeout(Duration::from_secs(25))
                    .await;

                let active_connection = match r {
                    Ok(Ok(r)) => r.active_connection_path,
                    Err(_) => {
                        warn!("Failed to create hotspot: Timeout. Trying to establish a connection instead.");
                        return Ok(Some(StateMachine::TryReconnect(config, nm)));
                    }
                    Ok(Err(e)) => {
                        warn!(
                            "Failed to create hotspot: {}. Trying to establish a connection instead.",
                            e
                        );
                        return Ok(Some(StateMachine::TryReconnect(config, nm)));
                    }
                };

                info!("Activating portal services");
                use super::portal::Portal;
                let (portal, exit_handler) = Portal::new(
                    &nm,
                    &config,
                    active_connection,
                    wifi_access_points,
                    Duration::from_secs(config.retry_in),
                )?;

                let r = portal.ctrl_c_exit(exit_handler).await;
                info!("Portal closed");
                match r {
                    // Ctrl+C
                    None => Ok(Some(StateMachine::Exit(nm))),
                    // Either the user has entered a wifi connection or a timeout happened
                    Some(wifi_connection) => {
                        match wifi_connection? {
                            // The user has entered a wifi connection
                            Some(wifi_connection) => Ok(Some(StateMachine::Connect(config, nm, wifi_connection))),
                            // Timeout
                            None => Ok(Some(StateMachine::TryReconnect(config, nm))),
                        }
                    }
                }
            }
            StateMachine::Connect(config, nm, network) => {
                info!("Connecting ...");

                let connection = nm
                    .connect_to(
                        network.ssid,
                        credentials_from_data(
                            network.passphrase.unwrap_or_default(),
                            network.identity,
                            network.mode.try_into()?,
                        )?,
                        network.hw,
                        true,
                    )
                    .await?;
                if let Some(connection) = connection {
                    match connection.state {
                        ConnectionState::Activated => Ok(Some(StateMachine::Connected(config, nm))),
                        _ => Ok(Some(StateMachine::ActivatePortal(config, nm))),
                    }
                } else {
                    Ok(Some(StateMachine::ActivatePortal(config, nm)))
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

/// Updates SSID/Password via passphrase file, if option is set in config.
///
/// This is not async, because current async file io implementations use the sync API with a background thread.
/// In that case we can do sync io as well (in the main executor thread) and are more efficient!
fn update_portal_info_via_file(config: &mut Config) {
    use std::fs::File;
    use std::io::BufReader;
    use std::io::BufRead;

    let file = config.passphrase_file.as_ref().and_then(|pfile| File::open(pfile).ok());
    if let Some(file) = file {
        let mut file = BufReader::new(file);
        let mut pwd = String::new();
        let mut ssid = String::new();
        let pwd_res = file.read_line(&mut pwd);
        let ssid_res = file.read_line(&mut ssid);
        match (pwd_res, ssid_res) {
            (Ok(_), Ok(_)) => {
                let pwd = pwd.trim_end();
                if verify_password(pwd).is_ok() {
                    info!("Updated portal password via file");
                    config.passphrase = pwd.to_owned();
                }
                if ssid.len() > 3 {
                    let ssid = ssid.trim_end();
                    info!("Updated portal ssid via file: {}", ssid);
                    config.ssid = ssid.to_owned();
                }
            }
            _ => warn!("Portal SSID/Password in file invalid!")
        }
    } else {
        warn!("Portal SSID/Password file given, but file is not readable!");
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::io::BufWriter;

    #[test]
    fn update_portal_info_via_file() {
        let mut config = super::Config::new();
        config.ssid="default".to_owned();
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let mut file = BufWriter::new(file);
        let _ = file.write_all(b"a_password");
        let _ = file.flush();

        // test with only one line: password
        config.passphrase_file = Some(path.clone());
        super::update_portal_info_via_file(&mut config);
        assert_eq!(&config.passphrase, "a_password");
        assert_eq!(&config.ssid, "default");

        let _ = file.write_all(b"\na_ssid");
        let _ = file.flush();

        // test with two lines: password ssid
        config.passphrase_file = Some(path);
        super::update_portal_info_via_file(&mut config);
        assert_eq!(&config.passphrase, "a_password");
        assert_eq!(&config.ssid, "a_ssid");
    }
}