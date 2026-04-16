use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

mod models;
mod config_handle;
mod simulator;
mod comms;

use config_handle::{DeviceTypeRegistry, PlantStore};
use simulator::{PlantHandle, PhysicsEngine, TickPlan, tick};

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
    // Load config
    // -------------------------------------------------------------------------
    tracing::info!("Loading device type registry from {}", device_types_path.display());
    let type_registry = DeviceTypeRegistry::load(device_types_path.to_str().unwrap())?;

    tracing::info!("Loading plant config from {}", factory_path.display());
    let plant_store = PlantStore::load(factory_path.to_str().unwrap())?;

    // -------------------------------------------------------------------------
    // Build runtime handle
    // -------------------------------------------------------------------------
    tracing::info!("Building factory handle");
    let handle: Arc<RwLock<PlantHandle>> = PlantHandle::new(type_registry, plant_store)?;

    // -------------------------------------------------------------------------
    // Compile physics scripts (fails fast on syntax errors)
    // -------------------------------------------------------------------------
    tracing::info!("Compiling physics scripts");
    let physics = {
        let h = handle.read().await;
        let device_types: Vec<_> = h.resolved_devices()
            .iter()
            .map(|d| d.type_def.clone())
            .collect();
        PhysicsEngine::new(&device_types)?
    };
    let physics = Arc::new(physics);

    // -------------------------------------------------------------------------
    // Build tick execution plan (topological sort — fails on wiring cycles)
    // -------------------------------------------------------------------------
    tracing::info!("Building tick plan");
    let plan = {
        let h = handle.read().await;
        TickPlan::build(&h)?
    };
    let plan = Arc::new(plan);

    tracing::info!("Startup complete — devices in tick order: {:?}", plan.order());

    // -------------------------------------------------------------------------
    // Spawn tick loop
    // -------------------------------------------------------------------------
    let tick_handle   = Arc::clone(&handle);
    let tick_physics  = Arc::clone(&physics);
    let tick_plan     = Arc::clone(&plan);

    tokio::spawn(async move {
        let default_tick_ms = {
            tick_handle.read().await.default_tick_ms()
        };
        let interval = Duration::from_millis(default_tick_ms);

        loop {
            let tick_start = Instant::now();

            {
                let mut h = tick_handle.write().await;
                tick(&mut h, &tick_plan, &tick_physics, interval.as_secs_f64());
            }

            // Sleep for whatever is left of the tick interval
            let elapsed = tick_start.elapsed();
            if let Some(remaining) = interval.checked_sub(elapsed) {
                tokio::time::sleep(remaining).await;
            } else {
                tracing::warn!("Tick overran by {:?}", elapsed - interval);
            }
        }
    });

    // -------------------------------------------------------------------------
    // -------------------------------------------------------------------------
    // Spawn comms layer
    // -------------------------------------------------------------------------

    // 1. PLC server — exposes simulated devices as OPC-UA endpoints
    comms::plc_server::start(Arc::clone(&handle)).await?;

    // 2. SCADA connectors — one thread per PLC endpoint, polls into IngestedState
    let ingested = comms::start_connectors(Arc::clone(&handle)).await?;

    // 3. TODO: ws_bridge — streams IngestedState to frontend
    //    comms::ws_bridge::start(Arc::clone(&ingested)).await?;
    // -------------------------------------------------------------------------

    // Keep main alive until Ctrl-C
    tracing::info!("Running — press Ctrl-C to stop");
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down");

    Ok(())
}
