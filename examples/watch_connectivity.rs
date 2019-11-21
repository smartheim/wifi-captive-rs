use wifi_captive::NetworkBackend;

use log::{info, LevelFilter};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::builder().filter_level(LevelFilter::Info).init();

    info!("Starting");
    let manager = NetworkBackend::new(&None).await?;

    manager.wait_for_connectivity(true, Duration::from_secs(20)).await?;
    info!("Connected");

    let nm_clone = manager.clone();
    tokio::spawn(async move {
        let _ = nm_clone.print_connectivity_changes().await;
    });

    manager.print_connection_changes().await?;

    Ok(())
}
