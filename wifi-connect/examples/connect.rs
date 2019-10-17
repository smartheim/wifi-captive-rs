pub mod shared;

use structopt::StructOpt;

use wifi_captive::lib::{credentials_from_data, NetworkManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: shared::Config = shared::Config::from_args();

    let manager = NetworkManager::new(&config.interface).await?;
    let state = manager
        .connect_to(
            config.ssid,
            credentials_from_data(config.passphrase, None, "wpa")?,
            None,
            true
        )
        .await?;

    match state {
        Some(_) => println!("Connected"),
        None => println!("Connection failed")
    }

    Ok(())
}
