use wifi_captive::lib::{NetworkManager, print_connection_changes};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = NetworkManager::new(&None).await?;
    print_connection_changes(&manager).await?;

    Ok(())
}
