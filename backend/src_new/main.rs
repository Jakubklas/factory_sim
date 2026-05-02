use tracing_subscriber::EnvFilter;

mod primitives;
mod config_handle;
mod simulator;
mod comms;
mod plant;
mod api;

use config_handle::load_all;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("info".parse()?)
                .add_directive("opcua=warn".parse()?)
        )
        .init();

    tracing::info!("=== water-plant-twin starting ===");

    let (app, handle) = load_all()?;

    let ingested    = plant::start(handle.clone(), app.tick_ms).await?;
    let ingested_ws = ingested.clone();
    let ws_host     = app.ws_host.clone();
    tokio::spawn(async move {
        if let Err(e) = api::ws_bridge::start(ingested_ws, app.tick_ms, &ws_host, app.ws_port).await {
            tracing::error!("WS bridge error: {}", e);
        }
    });

    tracing::info!("=== Running — press Ctrl-C to stop ===");
    tokio::signal::ctrl_c().await?;

    tracing::info!("Ctrl-C received — shutting down");
    let sim_ports: Vec<u16> = handle.read().await
        .all_plcs().iter()
        .filter(|p| p.simulated)
        .map(|p| p.port)
        .collect();
    if !sim_ports.is_empty() {
        comms::release_ports(&sim_ports);
    }
    tracing::info!("=== Shutdown complete ===");

    Ok(())
}
