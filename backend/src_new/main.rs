use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

mod models;
mod config_handle;
mod simulator;
mod comms;

use config_handle::{DeviceTypeRegistry, PlantRegistry, PlantConfigHandle};
use simulator::SimulatorModule;
use comms::{GenericConnector, IngestedState, ScadaPlcConnector};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // -------------------------------------------------------------------------
    // Logging
    // -------------------------------------------------------------------------
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("info".parse()?))
        .init();

    // -------------------------------------------------------------------------
    // Resolve config paths relative to the binary location
    // -------------------------------------------------------------------------
    let binary_dir = std::env::current_exe()?
        .parent()
        .expect("binary has no parent directory")
        .to_path_buf();
    let device_types_path = binary_dir.join("config/device_types.json");
    let factory_path      = binary_dir.join("config/factory.json");

    // -------------------------------------------------------------------------
    // Load config and build the plant config handle — source of truth for
    // both the simulator and the connector layer
    // -------------------------------------------------------------------------
    tracing::info!("Loading device type registry from {}", device_types_path.display());
    let type_registry = DeviceTypeRegistry::load(device_types_path.to_str().unwrap())?;

    tracing::info!("Loading plant config from {}", factory_path.display());
    let plant_registry = PlantRegistry::load(factory_path.to_str().unwrap())?;

    let handle = PlantConfigHandle::new(type_registry, plant_registry)?;

    // -------------------------------------------------------------------------
    // Spawn simulator — starts OPC-UA servers at the addresses the config
    // specifies. Skip this block to connect to real hardware instead.
    // -------------------------------------------------------------------------
    tracing::info!("Spawning simulator module");
    SimulatorModule::spawn(Arc::clone(&handle)).await?;

    // -------------------------------------------------------------------------
    // Start connectors — derived from the plant config, not the simulator.
    // Protocol field on each PLC selects the ConnectorImpl at compile time.
    // -------------------------------------------------------------------------
    let ingested: Arc<RwLock<IngestedState>> = Arc::new(RwLock::new(HashMap::new()));
    let endpoints = handle.read().await.endpoint_configs();
    let tick_ms   = handle.read().await.default_tick_ms();

    tracing::info!("Starting {} connector(s)", endpoints.len());
    for endpoint in endpoints {
        match endpoint.protocol.as_str() {
            "opcua" => {
                let (name, connector) = ScadaPlcConnector::new(endpoint);
                GenericConnector::new(name, connector, tick_ms, Arc::clone(&ingested)).start();
            }
            other => {
                tracing::warn!("Skipping PLC '{}': unknown protocol '{}'", endpoint.name, other);
            }
        }
    }

    // -------------------------------------------------------------------------
    // TODO: ws_bridge — streams IngestedState to frontend
    // comms::ws_bridge::start(Arc::clone(&ingested)).await?;
    // -------------------------------------------------------------------------

    tracing::info!("Running — press Ctrl-C to stop");
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down");

    Ok(())
}
