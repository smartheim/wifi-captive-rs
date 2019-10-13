//! This example
mod shared;

use structopt::StructOpt;
use std::net::Ipv4Addr;

use wifi_captive::nm::NetworkManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: shared::Config = shared::Config::from_args();

    let manager = NetworkManager::new(&config.interface).await?;
    manager.create_start_hotspot(config.ssid, config.passphrase, Some(Ipv4Addr::new(10, 0, 0, 1))).await?;

    Ok(())
}
