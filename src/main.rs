#![cfg_attr(feature = "external_doc", feature(external_doc))]
#![cfg_attr(feature = "external_doc", doc(include = "../readme.md"))]
#![feature(drain_filter)]

#[macro_use]
extern crate log;

use wifi_captive::*;

use env_logger::{Env, TimestampPrecision, DEFAULT_FILTER_ENV};
use std::io::ErrorKind;
use std::net::{SocketAddr, SocketAddrV4};
use structopt::StructOpt;

fn map_to_err(err_kind: ErrorKind, server_addr: SocketAddrV4, service_name: &'static str) -> CaptivePortalError {
    match err_kind {
        ErrorKind::AddrNotAvailable => CaptivePortalError::Generic(format!(
            "Could not bind to {:?} for {}\nThe gateway address is not assigned to any interface!",
            server_addr, service_name,
        )),
        ErrorKind::PermissionDenied => CaptivePortalError::Generic(format!(
            "You require elevated permissions to bind to port {} for {}.\n\
             You may use `sudo setcap CAP_NET_BIND_SERVICE=+eip {}`",
            server_addr.port(),
            service_name,
            std::env::args().next().unwrap_or_default()
        )),
        ErrorKind::AddrInUse => CaptivePortalError::Generic(format!(
            "Could not bind to port {} for {}\nThe port is in use.",
            server_addr.port(),
            service_name,
        )),
        _ => CaptivePortalError::Generic(format!(
            "Could not bind to {:?} for {}\nThis error happened: {:?}",
            server_addr, service_name, err_kind
        )),
    }
}

// Test if binding to the given address and port works
pub async fn test_udp(server_addr: SocketAddrV4, service_name: &'static str) -> Result<(), CaptivePortalError> {
    let socket = tokio::net::UdpSocket::bind(SocketAddr::V4(server_addr.clone()))
        .await
        .map_err(|e| map_to_err(e.kind(), server_addr, service_name))?;
    socket.set_broadcast(true)?;
    Ok(())
}

pub async fn test_tcp(server_addr: SocketAddrV4) -> Result<(), CaptivePortalError> {
    let socket = tokio::net::TcpListener::bind(SocketAddr::V4(server_addr.clone()))
        .await
        .map_err(|e| map_to_err(e.kind(), server_addr, "HTTP Web Interface"))?;
    drop(socket);
    Ok(())
}

#[tokio::main]
async fn main() {
    let mut builder = env_logger::Builder::from_env(Env::new().filter_or(DEFAULT_FILTER_ENV, "info"));
    builder
        .format_timestamp(Some(TimestampPrecision::Seconds))
        .format_module_path(false)
        .init();

    if let Err(e) = main_inner().await {
        error!("{}", e.to_string());
    }
}

async fn main_inner() -> Result<(), Box<dyn std::error::Error>> {
    let config: config::Config = config::Config::from_args();

    if config.passphrase.len() > 0 {
        verify_password(&config.passphrase)?;
    }

    test_udp(SocketAddrV4::new(config.gateway, config.dns_port), "DNS Server").await?;
    test_udp(SocketAddrV4::new(config.gateway, config.dhcp_port), "DHCP Server").await?;
    test_tcp(SocketAddrV4::new(config.gateway, config.listening_port)).await?;

    let mut sm = state_machine::StateMachine::StartUp(config.clone());

    loop {
        sm = if let Some(sm) = sm.progress().await? {
            sm
        } else {
            break;
        }
    }

    info!("State machine left");
    Ok(())
}
