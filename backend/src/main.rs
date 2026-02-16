mod simulator;
mod opcua_server;
mod ws_bridge;
mod models;

use simulator::plant::Plant;
use tokio::sync::{broadcast, RwLock};
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use models::{PlantConfig, DeviceHandle, DeviceSchemaRegistry, DeviceConfigRegistry, Topology};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    eprintln!("DEBUG: main() started");
    tracing::info!("Starting Water Plant Digital Twin with OPC UA Architecture");

    // Create broadcast channel for plant state updates
    let (tx, _rx) = broadcast::channel(100);
    let tx_ws = tx.clone();
    let tx_scada = tx.clone();

    // Load device schemas, device configs, and topology
    eprintln!("DEBUG: Loading device schemas from config/available_devices.json");
    let schema_registry = match DeviceSchemaRegistry::from_json("config/available_devices.json") {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("ERROR: Failed to load device schemas: {}", e);
            panic!("Failed to load device schemas: {}", e);
        }
    };

    eprintln!("DEBUG: Loading device configs from config/devices.json");
    let device_configs = match DeviceConfigRegistry::from_json("config/devices.json") {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("ERROR: Failed to load device configs: {}", e);
            panic!("Failed to load device configs: {}", e);
        }
    };

    eprintln!("DEBUG: Loading topology from config/topology.json");
    let topology = match Topology::from_json("config/topology.json") {
        Ok(topology) => topology,
        Err(e) => {
            eprintln!("ERROR: Failed to load topology: {}", e);
            panic!("Failed to load topology: {}", e);
        }
    };

    // Create plant from configuration
    eprintln!("DEBUG: Creating plant from configuration");
    let plant = match Plant::from_config(device_configs.devices, topology, &schema_registry) {
        Ok(plant) => Arc::new(RwLock::new(plant)),
        Err(e) => {
            eprintln!("ERROR: Failed to create plant: {}", e);
            panic!("Failed to create plant: {}", e);
        }
    };
    eprintln!("DEBUG: Plant created successfully");

    // Start WebSocket server
    let ws_server = tokio::spawn(async move {
        if let Err(e) = ws_bridge::start_ws_server(tx_ws).await {
            tracing::error!("WebSocket server error: {}", e);
        }
    });

    // Create device handles from plant state
    eprintln!("DEBUG: Creating device handles");
    let mut device_handles: HashMap<String, DeviceHandle> = {
        let plant_state = plant.read().await.get_state();
        plant_state
            .devices
            .into_iter()
            .map(|(id, device)| {
                let device_arc = Arc::new(RwLock::new(device));
                (id, DeviceHandle::new(device_arc))
            })
            .collect()
    };
    eprintln!("DEBUG: Created {} device handles", device_handles.len());

    // Clone device handles for sync task
    let device_handles_sync = device_handles.clone();
    let plant_sync = plant.clone();

    // Spawn task to sync devices from plant to device handles
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            let plant = plant_sync.read().await;

            for (device_id, handle) in &device_handles_sync {
                if let Some(device) = plant.get_device(device_id) {
                    *handle.get_device().write().await = device.clone();
                }
            }
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

        // Map device IDs to DeviceHandles (now dynamic, no hardcoded match!)
        eprintln!("DEBUG: Mapping {} devices for {}", plc_config.device_mappings.len(), plc_config.name);
        for device_mapping in &plc_config.device_mappings {
            eprintln!("DEBUG: Mapping device: {}", device_mapping.device_id);
            if let Some(handle) = device_handles.get(&device_mapping.device_id) {
                devices.insert(device_mapping.device_id.clone(), handle.clone());
            } else {
                tracing::warn!("Device '{}' not found in plant", device_mapping.device_id);
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
