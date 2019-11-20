#![cfg_attr(feature = "external_doc", feature(external_doc))]
#![cfg_attr(feature = "external_doc", doc(include = "../readme.md"))]
#![feature(drain_filter)]

#[macro_use]
extern crate log;

use wifi_captive::*;

use env_logger::Env;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use structopt::StructOpt;

// Test if binding to the given address and port works
pub async fn test_udp(server_addr: SocketAddrV4) -> Result<(), CaptivePortalError> {
    let socket = tokio::net::UdpSocket::bind(SocketAddr::V4(server_addr.clone()))
        .await
        .map_err(|_| {
            CaptivePortalError::GenericO(format!(
                "Could not bind to {:?}\nEither the port is blocked or permissions are required.\n\
                 You may use `sudo setcap CAP_NET_BIND_SERVICE=+eip {}`",
                server_addr,
                std::env::args().next().unwrap_or_default()
            ))
        })?;
    socket.set_broadcast(true)?;
    Ok(())
}

pub async fn test_tcp(server_addr: SocketAddrV4) -> Result<(), CaptivePortalError> {
    let socket = tokio::net::TcpListener::bind(SocketAddr::V4(server_addr.clone()))
        .await
        .map_err(|_| {
            CaptivePortalError::GenericO(format!("Could not bind to {:?}", server_addr))
        })?;
    drop(socket);
    Ok(())
}

#[tokio::main]
async fn main() {
    let env = Env::new().filter_or("RUST_LOG", "info");
    env_logger::init_from_env(env);

    if let Err(e) = main_inner().await {
        error!("{}", e.to_string());
    }
}

async fn main_inner() -> Result<(), Box<dyn std::error::Error>> {
    let config: config::Config = config::Config::from_args();

    test_udp(SocketAddrV4::new(
        Ipv4Addr::new(127, 0, 0, 1),
        config.dns_port,
    ))
    .await?;
    test_udp(SocketAddrV4::new(
        Ipv4Addr::new(127, 0, 0, 1),
        config.dhcp_port,
    ))
    .await?;
    test_tcp(SocketAddrV4::new(
        Ipv4Addr::new(127, 0, 0, 1),
        config.listening_port,
    ))
    .await?;

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
