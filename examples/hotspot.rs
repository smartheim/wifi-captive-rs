//! This example
pub mod shared;

use std::net::Ipv4Addr;
use structopt::StructOpt;

use wifi_captive::NetworkBackend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: shared::Config = shared::Config::from_args();

    let manager = NetworkBackend::new(&config.interface).await?;
    manager
        .hotspot_start(config.ssid, config.passphrase, Some(Ipv4Addr::new(10, 0, 0, 1)))
        .await?;

    Ok(())
}
