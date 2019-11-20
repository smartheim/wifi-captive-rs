pub mod shared;

use structopt::StructOpt;

use wifi_captive::{credentials_from_data, NetworkBackend, Security};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: shared::Config = shared::Config::from_args();

    let manager = NetworkBackend::new(&config.interface).await?;
    let state = manager
        .connect_to(
            config.ssid,
            credentials_from_data(config.passphrase, None, Security::WPA2)?,
            None,
            true,
        )
        .await?;

    match state {
        Some(_) => println!("Connected"),
        None => println!("Connection failed"),
    }

    Ok(())
}
