use wifi_captive::nm::{NetworkManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let manager = NetworkManager::new(&None).await?;
    manager.print_connection_changes().await?;

    Ok(())
}
