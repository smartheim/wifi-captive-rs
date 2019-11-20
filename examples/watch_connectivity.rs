use wifi_captive::NetworkBackend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = NetworkBackend::new(&None).await?;
    manager.print_connection_changes().await?;

    Ok(())
}
