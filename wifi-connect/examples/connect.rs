pub mod shared;

use structopt::StructOpt;

use wifi_captive::nm::{NetworkManager,credentials_from_data};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: shared::Config = shared::Config::from_args();

    let manager = NetworkManager::new(&config.interface).await?;
    manager.connect_to(config.ssid, None, credentials_from_data(config.passphrase, None, "wpa")?).await?;

    Ok(())
}
