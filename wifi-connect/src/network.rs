use std::process;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

use log::{debug, error};
use serde::{Deserialize, Serialize};

use std::net::Ipv4Addr;

use super::network_manager::{
    AccessPoint, AccessPointCredentials, Connection, ConnectionState, Connectivity, Device,
    DeviceType, ServiceState, errors::NetworkManagerError, Security,
    get_service_state, start_service,
};

use super::Config;
use super::dnsmasq::start_dnsmasq;
use super::exit::trap_exit_signals;
use super::server::start_server;
use crate::network_manager::get_connections;

type ExitResult = Result<(), super::network_manager::errors::NetworkManagerError>;

pub enum NetworkCommand {
    Activate,
    Timeout,
    Exit,
    Connect {
        ssid: String,
        identity: String,
        passphrase: String,
    },
}

pub fn exit(exit_tx: &Sender<Result<(), NetworkManagerError>>, error: NetworkManagerError) {
    let _ = exit_tx.send(Err(error));
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Network {
    ssid: String,
    security: String,
}

pub enum NetworkCommandResponse {
    Networks(Vec<Network>),
}

struct NetworkCommandHandler {
    manager: NetworkManager,
    device: Device,
    access_points: Vec<AccessPoint>,
    portal_connection: Option<Connection>,
    config: Config,
    dnsmasq: process::Child,
    server_tx: Sender<NetworkCommandResponse>,
    network_rx: Receiver<NetworkCommand>,
    activated: bool,
}

impl NetworkCommandHandler {
    fn new(config: Config) -> Result<Self, NetworkManagerError> {
        let (network_tx, network_rx) = channel();

        let manager = NetworkManager::new();
        debug!("network_manager connection initialized");

        let device = find_device(&manager, &config.interface)?;
        let access_points = get_access_points(&device)?;
        let portal_connection = Some(create_portal(&device, &config)?);
        let dnsmasq = start_dnsmasq(&config, &device)
            .map_err(|e| NetworkManagerError::from(e.to_string()))?;
        let (server_tx, server_rx) = channel();
        Self::spawn_server(&config, server_rx, network_tx.clone());
        Self::spawn_activity_timeout(&config, network_tx.clone());

        // let config = config.clone();
        let activated = false;

        Ok(NetworkCommandHandler {
            manager,
            device,
            access_points,
            portal_connection,
            config,
            dnsmasq,
            server_tx,
            network_rx,
            activated,
        })
    }

    fn spawn_server(
        config: &Config,
        server_rx: Receiver<NetworkCommandResponse>,
        network_tx: Sender<NetworkCommand>,
    ) {
        let gateway = config.gateway;
        let listening_port = config.listening_port;
        let ui_directory = config.ui_directory.clone();

        thread::spawn(move || {
            start_server(
                gateway,
                listening_port,
                server_rx,
                network_tx,
                &ui_directory,
            );
        });
    }

    fn spawn_activity_timeout(config: &Config, network_tx: Sender<NetworkCommand>) {
        let activity_timeout = config.activity_timeout;

        if activity_timeout == 0 {
            return;
        }

        thread::spawn(move || {
            use std::error::Error;

            thread::sleep(Duration::from_secs(activity_timeout));

            if let Err(err) = network_tx.send(NetworkCommand::Timeout) {
                error!(
                    "Sending NetworkCommand::Timeout failed: {}",
                    err.description()
                );
            }
        });
    }

    fn spawn_trap_exit_signals(exit_tx: &Sender<ExitResult>, network_tx: Sender<NetworkCommand>) {
        let exit_tx_trap = exit_tx.clone();

        thread::spawn(move || {
            use std::error::Error;
            if let Err(e) = trap_exit_signals() {
                exit(&exit_tx_trap, e);
                return;
            }

            if let Err(err) = network_tx.send(NetworkCommand::Exit) {
                error!("Sending NetworkCommand::Exit failed: {}", err.description());
            }
        });
    }

    fn run(&mut self) {
        let result = self.run_loop();
        self.stop(result.map_err(|e| e.into()));
    }

    fn run_loop(&mut self) -> Result<(), NetworkManagerError> {
        loop {
            let command = self.receive_network_command()?;

            match command {
                NetworkCommand::Activate => {
                    self.activate()?;
                }
                NetworkCommand::Timeout => {
                    if !self.activated {
                        info!("Timeout reached. Exiting...");
                        return Ok(());
                    }
                }
                NetworkCommand::Exit => {
                    info!("Exiting...");
                    return Ok(());
                }
                NetworkCommand::Connect {
                    ssid,
                    identity,
                    passphrase,
                } => {
                    if self.connect(&ssid, &identity, &passphrase)? {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn receive_network_command(&self) -> Result<NetworkCommand, NetworkManagerError> {
        match self.network_rx.recv() {
            Ok(command) => Ok(command),
            Err(e) => {
                // Sleep for a second, so that other threads may log error info.
                thread::sleep(Duration::from_secs(1));
                Err(e.into())
            }
        }
    }

    fn stop(&mut self, result: ExitResult) {
        let _ = self.dnsmasq.kill();

        if let Some(ref connection) = self.portal_connection {
            let _ = stop_portal_impl(connection, &self.config);
        }

        //  let _ = exit_tx.send(result);
    }

    fn activate(&mut self) -> Result<(), NetworkManagerError> {
        self.activated = true;

        let networks = get_networks(&self.access_points);

        self.server_tx.send(NetworkCommandResponse::Networks(networks))
            .map_err(|_e| NetworkManagerError::Generic("NetworkCommandResponse"))
    }

    fn connect(&mut self, ssid: &str, identity: &str, passphrase: &str) -> Result<bool, NetworkManagerError> {
        delete_connection_if_exists(&self.manager, ssid);

        if let Some(ref connection) = self.portal_connection {
            stop_portal(connection, &self.config)?;
        }

        self.portal_connection = None;

        self.access_points = get_access_points(&self.device)?;

        if let Some(access_point) = find_access_point(&self.access_points, ssid) {
            let wifi_device = self.device.as_wifi_device().unwrap();

            info!("Connecting to access point '{}'...", ssid);

            let credentials = init_access_point_credentials(access_point, identity, passphrase);

            match wifi_device.connect(access_point, &credentials) {
                Ok((connection, state)) => {
                    if state == ConnectionState::Activated {
                        match wait_for_connectivity(&self.manager, 20) {
                            Ok(has_connectivity) => {
                                if has_connectivity {
                                    info!("Internet connectivity established");
                                } else {
                                    warn!("Cannot establish Internet connectivity");
                                }
                            }
                            Err(err) => error!("Getting Internet connectivity failed: {}", err),
                        }

                        return Ok(true);
                    }

                    if let Err(err) = connection.delete() {
                        error!("Deleting connection object failed: {}", err)
                    }

                    warn!(
                        "Connection to access point not activated '{}': {:?}",
                        ssid, state
                    );
                }
                Err(e) => {
                    warn!("Error connecting to access point '{}': {}", ssid, e);
                }
            }
        }

        self.access_points = get_access_points(&self.device)?;

        self.portal_connection = Some(create_portal(&self.device, &self.config)?);

        Ok(false)
    }
}

fn init_access_point_credentials(
    access_point: &AccessPoint,
    identity: &str,
    passphrase: &str,
) -> AccessPointCredentials {
    if access_point.security.contains(Security::ENTERPRISE) {
        AccessPointCredentials::Enterprise {
            identity: identity.to_string(),
            passphrase: passphrase.to_string(),
        }
    } else if access_point.security.contains(Security::WPA2)
        || access_point.security.contains(Security::WPA)
    {
        AccessPointCredentials::Wpa {
            passphrase: passphrase.to_string(),
        }
    } else if access_point.security.contains(Security::WEP) {
        AccessPointCredentials::Wep {
            passphrase: passphrase.to_string(),
        }
    } else {
        AccessPointCredentials::None
    }
}

pub fn process_network_commands(config: Config) {
    let mut command_handler = match NetworkCommandHandler::new(config) {
        Ok(command_handler) => command_handler,
        Err(e) => {
            //exit(exit_tx, e);
            return;
        }
    };

    command_handler.run();
}

pub fn find_device(manager: &NetworkManager, interface: &Option<String>) -> Result<Device, NetworkManagerError> {
    if let Some(ref interface) = *interface {
        let device = manager.get_device_by_interface(interface)?;

        if *device.device_type() == DeviceType::WiFi {
            info!("Targeted WiFi device: {}", interface);
            Ok(device)
        } else {
            Err(NetworkManagerError::not_awi_fi_device(interface.clone()))
        }
    } else {
        let devices = manager.get_devices()?;

        let index = devices
            .iter()
            .position(|d| *d.device_type() == DeviceType::WiFi);

        if let Some(index) = index {
            info!("WiFi device: {}", devices[index].interface());
            Ok(devices[index].clone())
        } else {
            Err(NetworkManagerError::no_wi_fi_device())
        }
    }
}

fn get_access_points(device: &Device) -> Result<Vec<AccessPoint>, NetworkManagerError> {
    get_access_points_impl(device)
}

fn get_access_points_impl(device: &Device) -> Result<Vec<AccessPoint>, NetworkManagerError> {
    let retries_allowed = 10;
    let mut retries = 0;

    // After stopping the hotspot we may have to wait a bit for the list
    // of access points to become available
    while retries < retries_allowed {
        let wifi_device = device.as_wifi_device().unwrap();
        let mut access_points = wifi_device.get_access_points()?;

        access_points.retain(|ap| ap.ssid().as_str().is_ok());

        if !access_points.is_empty() {
            info!(
                "Access points: {:?}",
                get_access_points_ssids(&access_points)
            );
            return Ok(access_points);
        }

        retries += 1;
        debug!("No access points found - retry #{}", retries);
        thread::sleep(Duration::from_secs(1));
    }

    warn!("No access points found - giving up...");
    Ok(vec![])
}

fn get_access_points_ssids(access_points: &[AccessPoint]) -> Vec<&str> {
    access_points
        .iter()
        .map(|ap| ap.ssid().as_str().unwrap())
        .collect()
}

fn get_networks(access_points: &[AccessPoint]) -> Vec<Network> {
    access_points
        .iter()
        .map(|ap| get_network_info(ap))
        .collect()
}

fn get_network_info(access_point: &AccessPoint) -> Network {
    Network {
        ssid: access_point.ssid().as_str().unwrap().to_string(),
        security: get_network_security(access_point).to_string(),
    }
}

fn get_network_security(access_point: &AccessPoint) -> &str {
    if access_point.security.contains(Security::ENTERPRISE) {
        "enterprise"
    } else if access_point.security.contains(Security::WPA2)
        || access_point.security.contains(Security::WPA)
    {
        "wpa"
    } else if access_point.security.contains(Security::WEP) {
        "wep"
    } else {
        "none"
    }
}

fn find_access_point<'a>(access_points: &'a [AccessPoint], ssid: &str) -> Option<&'a AccessPoint> {
    for access_point in access_points.iter() {
        if let Ok(access_point_ssid) = access_point.ssid().as_str() {
            if access_point_ssid == ssid {
                return Some(access_point);
            }
        }
    }

    None
}

fn create_portal(device: &Device, config: &Config) -> Result<Connection, NetworkManagerError> {
    let portal_passphrase = config.passphrase.as_ref().map(|p| p as &str);
    create_portal_impl(device, &config.ssid, &config.gateway, &portal_passphrase)
}

fn create_portal_impl(
    device: &Device,
    ssid: &str,
    gateway: &Ipv4Addr,
    passphrase: &Option<&str>,
) -> Result<Connection, NetworkManagerError> {
    info!("Starting access point...");
    let wifi_device = device.as_wifi_device().unwrap();
    let (portal_connection, _) = wifi_device.create_hotspot(ssid, *passphrase, Some(*gateway))?;
    info!("Access point '{}' created", ssid);
    Ok(portal_connection)
}

fn stop_portal(connection: &Connection, config: &Config) -> Result<(), NetworkManagerError> {
    stop_portal_impl(connection, config)
}

fn stop_portal_impl(connection: &Connection, config: &Config) -> Result<(), NetworkManagerError> {
    info!("Stopping access point '{}'...", config.ssid);
    connection.deactivate()?;
    connection.delete()?;
    thread::sleep(Duration::from_secs(1));
    info!("Access point '{}' stopped", config.ssid);
    Ok(())
}

fn wait_for_connectivity(manager: &NetworkManager, timeout: u64) -> Result<bool, NetworkManagerError> {
    let mut total_time = 0;

    loop {
        let connectivity = manager.get_connectivity()?;

        if connectivity == Connectivity::Full || connectivity == Connectivity::Limited {
            debug!(
                "Connectivity established: {:?} / {}s elapsed",
                connectivity, total_time
            );

            return Ok(true);
        } else if total_time >= timeout {
            debug!(
                "Timeout reached in waiting for connectivity: {:?} / {}s elapsed",
                connectivity, total_time
            );

            return Ok(false);
        }

        ::std::thread::sleep(::std::time::Duration::from_secs(1));

        total_time += 1;

        debug!(
            "Still waiting for connectivity: {:?} / {}s elapsed",
            connectivity, total_time
        );
    }
}

pub fn start_network_manager_service() -> Result<(), NetworkManagerError> {
    let state = match get_service_state() {
        Ok(state) => state,
        _ => {
            info!("Cannot get the network_manager service state");
            return Ok(());
        }
    };

    if state != ServiceState::Active {
        let state = start_service(15)?;
        if state != ServiceState::Active {
            return Err(NetworkManagerError::start_active_network_manager());
        } else {
            info!("network_manager service started successfully");
        }
    } else {
        debug!("network_manager service already running");
    }

    Ok(())
}

pub fn delete_access_point_profiles() -> Result<(), NetworkManagerError> {
    let manager = NetworkManager::new();

    let connections = get_connections(&manager.dbus_manager)?;

    for connection in connections {
        if &connection.settings.kind == "802-11-wireless" && &connection.settings.mode == "ap" {
            debug!(
                "Deleting access point connection profile: {:?}",
                connection.settings.ssid,
            );
            connection.delete()?;
        }
    }

    Ok(())
}

fn delete_connection_if_exists(manager: &NetworkManager, ssid: &str) {
    let connections = match get_connections(&manager.dbus_manager) {
        Ok(connections) => connections,
        Err(e) => {
            error!("Getting existing connections failed: {}", e);
            return;
        }
    };

    for connection in connections {
        if let Ok(connection_ssid) = connection.settings.ssid.as_str() {
            if &connection.settings.kind == "802-11-wireless" && connection_ssid == ssid {
                info!(
                    "Deleting existing WiFi connection: {:?}",
                    connection.settings.ssid,
                );

                if let Err(e) = connection.delete() {
                    error!("Deleting existing WiFi connection failed: {}", e);
                }
            }
        }
    }
}
