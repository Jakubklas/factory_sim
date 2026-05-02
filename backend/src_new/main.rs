use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

mod primitives;
mod config_handle;
mod simulator;
mod comms;

use config_handle::{DeviceTypeRegistry, PlantRegistry, PlantConfigHandle};
use simulator::SimulatorModule;
use comms::{GenericConnector, IngestedState, ScadaPlcConnector, release_ports};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // -------------------------------------------------------------------------
    // Logging — RUST_LOG env var overrides the default directives.
    // e.g. RUST_LOG=water_plant_twin=debug,opcua=warn cargo run
    // -------------------------------------------------------------------------
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("info".parse()?)
                .add_directive("opcua=warn".parse()?)  // suppress OPC-UA protocol noise
        )
        .init();

    tracing::info!("=== water-plant-twin starting ===");

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
    // Per-PLC startup: simulated PLCs get a simulator server, all PLCs get
    // a connector. Physics tick loop starts once if any PLCs are simulated.
    // -------------------------------------------------------------------------
    let plcs      = handle.read().await.all_plcs().to_vec();
    let endpoints = handle.read().await.endpoint_configs();
    let tick_ms   = handle.read().await.default_tick_ms();

    if plcs.iter().any(|p| p.simulated) {
        SimulatorModule::start_physics(Arc::clone(&handle)).await?;
    }

    let ingested: Arc<RwLock<IngestedState>> = Arc::new(RwLock::new(HashMap::new()));

    for (plc, endpoint) in plcs.into_iter().zip(endpoints) {
        if plc.simulated {
            release_ports(&[plc.port]);
            SimulatorModule::start_server(Arc::clone(&handle), plc).await?;
        }

        match endpoint.protocol.as_str() {
            "opcua" => {
                tracing::info!("[opcua] {} → {}", endpoint.name, endpoint.url);
                let (name, connector) = ScadaPlcConnector::new(endpoint);
                GenericConnector::new(name, connector, tick_ms, Arc::clone(&ingested)).start();
            }
            other => {
                tracing::warn!("Skipping '{}': unknown protocol '{}'", endpoint.name, other);
            }
        }
    }

    // -------------------------------------------------------------------------
    // TODO: ws_bridge — streams IngestedState to frontend
    // comms::ws_bridge::start(Arc::clone(&ingested)).await?;
    // -------------------------------------------------------------------------

    tracing::info!("=== Running — press Ctrl-C to stop ===");
    tokio::signal::ctrl_c().await?;

    tracing::info!("Ctrl-C received — shutting down");
    let sim_ports: Vec<u16> = handle.read().await
        .all_plcs().iter()
        .filter(|p| p.simulated)
        .map(|p| p.port)
        .collect();
    if !sim_ports.is_empty() { release_ports(&sim_ports); }
    tracing::info!("=== Shutdown complete ===");

    Ok(())
}
