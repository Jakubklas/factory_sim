mod simulator;
mod opcua_server;
mod ws_bridge;
mod models;

use simulator::plant::Plant;
use tokio::sync::{broadcast, RwLock};
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use models::{PlantConfig, DeviceHandle};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    eprintln!("DEBUG: main() started");
    tracing::info!("Starting Water Plant Digital Twin with OPC UA Architecture");

    // Create broadcast channel for plant state updates
    let (tx, _rx) = broadcast::channel(100);
    let tx_ws = tx.clone();
    let tx_scada = tx.clone();

    // Create plant with Arc<RwLock> for shared access
    let plant = Arc::new(RwLock::new(Plant::new()));

    // Get references to individual devices for PLC servers
    let plant_clone = plant.clone();

    // Start WebSocket server
    let ws_server = tokio::spawn(async move {
        if let Err(e) = ws_bridge::start_ws_server(tx_ws).await {
            tracing::error!("WebSocket server error: {}", e);
        }
    });

    // Run simulation loop - updates device structs in memory
    let plant_sim = plant.clone();
    let simulation = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        tracing::info!("Simulation loop started");

        loop {
            interval.tick().await;

            // Update plant simulation (dt = 0.1 seconds)
            let mut plant = plant_sim.write().await;
            plant.tick(0.1);
        }
    });

    // Create Arc<RwLock> wrappers for each device
    let boiler1 = Arc::new(RwLock::new({
        let plant = plant.read().await;
        plant.boiler_1.clone()
    }));
    let boiler2 = Arc::new(RwLock::new({
        let plant = plant.read().await;
        plant.boiler_2.clone()
    }));
    let pressure_meter1 = Arc::new(RwLock::new({
        let plant = plant.read().await;
        plant.pressure_meter_1.clone()
    }));
    let valve1 = Arc::new(RwLock::new({
        let plant = plant.read().await;
        plant.valve_1.clone()
    }));
    let flow_meter1 = Arc::new(RwLock::new({
        let plant = plant.read().await;
        plant.flow_meter_1.clone()
    }));

    // Spawn task to sync devices from plant to individual Arc<RwLock> refs
    let plant_sync = plant.clone();
    let b1_sync = boiler1.clone();
    let b2_sync = boiler2.clone();
    let pm1_sync = pressure_meter1.clone();
    let v1_sync = valve1.clone();
    let fm1_sync = flow_meter1.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            let plant = plant_sync.read().await;

            *b1_sync.write().await = plant.boiler_1.clone();
            *b2_sync.write().await = plant.boiler_2.clone();
            *pm1_sync.write().await = plant.pressure_meter_1.clone();
            *v1_sync.write().await = plant.valve_1.clone();
            *fm1_sync.write().await = plant.flow_meter_1.clone();
        }
    });

    // Load factory configuration from JSON
    eprintln!("DEBUG: Loading config from config/factory.json");
    let plant_config = match PlantConfig::from_json("config/factory.json") {
        Ok(config) => {
            eprintln!("DEBUG: Config loaded successfully");
            config
        }
        Err(e) => {
            eprintln!("ERROR: Failed to load factory configuration: {}", e);
            eprintln!("Current directory: {:?}", std::env::current_dir());
            panic!("Failed to load factory configuration: {}", e);
        }
    };

    eprintln!("DEBUG: About to log PLC count");
    tracing::info!("Loaded configuration for {} PLCs", plant_config.plcs.len());
    eprintln!("DEBUG: Logged PLC count");

    // Start PLC servers dynamically based on configuration
    let mut plc_tasks = Vec::new();

    eprintln!("DEBUG: Starting PLC server loop");
    eprintln!("DEBUG: plant_config.plcs.len() = {}", plant_config.plcs.len());
    for (idx, plc_config) in plant_config.plcs.iter().enumerate() {
        eprintln!("DEBUG: Processing PLC #{}: {}", idx, plc_config.name);
        let mut devices = HashMap::new();

        // Map device IDs to DeviceHandles
        eprintln!("DEBUG: Mapping {} devices for {}", plc_config.device_mappings.len(), plc_config.name);
        for device_mapping in &plc_config.device_mappings {
            eprintln!("DEBUG: Mapping device: {}", device_mapping.device_id);
            let handle = match device_mapping.device_id.as_str() {
                "boiler-1" => Some(DeviceHandle::Boiler(boiler1.clone())),
                "boiler-2" => Some(DeviceHandle::Boiler(boiler2.clone())),
                "pressure-meter-1" => Some(DeviceHandle::PressureMeter(pressure_meter1.clone())),
                "valve-1" => Some(DeviceHandle::Valve(valve1.clone())),
                "flow-meter-1" => Some(DeviceHandle::FlowMeter(flow_meter1.clone())),
                _ => {
                    tracing::warn!("Unknown device ID: {}", device_mapping.device_id);
                    None
                }
            };

            if let Some(h) = handle {
                devices.insert(device_mapping.device_id.clone(), h);
            }
        }

        eprintln!("DEBUG: Device mapping complete for {}. Creating tokio task...", plc_config.name);
        let config = plc_config.clone();
        eprintln!("DEBUG: About to spawn PLC server task for {}", config.name);
        tracing::info!("Spawning PLC server task for {}", config.name);
        let plc_task = tokio::spawn(async move {
            tracing::info!("PLC server task starting for {}", config.name);
            if let Err(e) = opcua_server::plc_server::start_plc_server(config.clone(), devices).await {
                tracing::error!("{} server error: {}", config.name, e);
            }
        });

        plc_tasks.push(plc_task);
        eprintln!("DEBUG: PLC task pushed to plc_tasks vector");
    }

    eprintln!("DEBUG: Exited PLC server loop. Total PLC tasks: {}", plc_tasks.len());

    // Wait for PLC servers to start - they run in std::thread so need time to initialize
    eprintln!("DEBUG: About to sleep for 10 seconds to let PLC servers initialize");
    tracing::info!("Waiting for PLC servers to initialize...");
    tokio::time::sleep(Duration::from_secs(10)).await;
    eprintln!("DEBUG: Finished sleeping");
    tracing::info!("PLC servers should be ready now");

    // Start SCADA client (connects to all PLCs and aggregates data)
    // SCADA client runs in its own thread, so we don't need to await it
    let configs_for_scada = plant_config.plcs.clone();
    if let Err(e) = opcua_server::start_scada_client(tx_scada, configs_for_scada).await {
        tracing::error!("SCADA client error: {}", e);
    }
    eprintln!("DEBUG: SCADA client started");

    tracing::info!("Backend initialized:");
    for plc_config in &plant_config.plcs {
        tracing::info!("  - {} OPC UA Server on port {}", plc_config.name, plc_config.port);
    }
    tracing::info!("  - SCADA Client aggregating from {} PLCs", plant_config.plcs.len());
    tracing::info!("  - WebSocket on port 3000");

    // Wait for Ctrl+C or task completion
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl+C, shutting down");
        }
        _ = ws_server => {
            tracing::info!("WebSocket server terminated");
        }
        _ = simulation => {
            tracing::info!("Simulation terminated");
        }
    }

    tracing::info!("Shutting down");
}
