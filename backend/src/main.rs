mod simulator;
mod opcua_server;
mod ws_bridge;
mod models;

use simulator::plant::Plant;
use tokio::sync::{broadcast, RwLock};
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use models::{PlantConfig, DeviceHandle, DeviceSchemaRegistry, DeviceConfigRegistry, Topology, DeviceFieldValue};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    tracing::info!("Starting Water Plant Digital Twin with OPC UA Architecture");

    // Create broadcast channel for plant state updates
    let (tx, _rx) = broadcast::channel(100);
    let tx_ws = tx.clone();
    let tx_scada = tx.clone();

    // Load device schemas, device configs, and topology
    tracing::info!("Loading device schemas from config/available_devices.json");
    let schema_registry = match DeviceSchemaRegistry::from_json("config/available_devices.json") {
        Ok(registry) => registry,
        Err(e) => {
            tracing::error!("Failed to load device schemas: {}", e);
            panic!("Failed to load device schemas: {}", e);
        }
    };

    tracing::info!("Loading device configs from config/devices.json");
    let device_configs = match DeviceConfigRegistry::from_json("config/devices.json") {
        Ok(registry) => registry,
        Err(e) => {
            tracing::error!("Failed to load device configs: {}", e);
            panic!("Failed to load device configs: {}", e);
        }
    };

    tracing::info!("Loading topology from config/topology.json");
    let topology = match Topology::from_json("config/topology.json") {
        Ok(topology) => topology,
        Err(e) => {
            tracing::error!("Failed to load topology: {}", e);
            panic!("Failed to load topology: {}", e);
        }
    };

    // Create plant from configuration
    tracing::info!("Creating plant from configuration");
    let plant = match Plant::from_config(device_configs.devices, topology, &schema_registry) {
        Ok(plant) => Arc::new(RwLock::new(plant)),
        Err(e) => {
            tracing::error!("Failed to create plant: {}", e);
            panic!("Failed to create plant: {}", e);
        }
    };
    tracing::info!("Plant created successfully");

    // Start WebSocket server
    let ws_server = tokio::spawn(async move {
        if let Err(e) = ws_bridge::start_ws_server(tx_ws).await {
            tracing::error!("WebSocket server error: {}", e);
        }
    });

    // Create device handles from plant devices
    let device_handles: HashMap<String, DeviceHandle> = {
        let plant_locked = plant.read().await;
        plant_locked
            .get_devices()
            .iter()
            .map(|(id, device)| {
                let device_arc = Arc::new(RwLock::new(device.clone()));
                (id.clone(), DeviceHandle::new(device_arc))
            })
            .collect()
    };
    tracing::info!("Created {} device handles: [{}]", device_handles.len(), device_handles.keys().cloned().collect::<Vec<_>>().join(", "));

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

    // Periodic heartbeat: log plant state every 5 seconds
    let plant_heartbeat = plant.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let plant = plant_heartbeat.read().await;
            let state = plant.get_state();

            let mut lines: Vec<String> = state.devices.iter()
                .map(|(device_id, fields)| {
                    let summary: Vec<String> = fields.iter()
                        .map(|(k, v)| match v {
                            DeviceFieldValue::Float(f) => format!("{}={:.2}", k, f),
                            DeviceFieldValue::String(s) => format!("{}={}", k, s),
                        })
                        .collect();
                    format!("  {}: {}", device_id, summary.join(", "))
                })
                .collect();
            lines.sort();
            tracing::info!("Plant heartbeat ({} devices):\n{}", state.devices.len(), lines.join("\n"));
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
    tracing::info!("Loading factory config from config/factory.json");
    let plant_config = match PlantConfig::from_json("config/factory.json") {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Failed to load factory configuration: {} (cwd: {:?})", e, std::env::current_dir());
            panic!("Failed to load factory configuration: {}", e);
        }
    };

    tracing::info!("Loaded configuration for {} PLCs", plant_config.plcs.len());

    // Start PLC servers dynamically based on configuration
    let mut plc_tasks = Vec::new();

    for plc_config in plant_config.plcs.iter() {
        let mut devices = HashMap::new();

        // Map device IDs to DeviceHandles (now dynamic, no hardcoded match!)
        let device_ids: Vec<_> = plc_config.device_mappings.iter().map(|m| m.device_id.as_str()).collect();
        tracing::info!("Mapping {} devices for {}: [{}]", device_ids.len(), plc_config.name, device_ids.join(", "));

        for device_mapping in &plc_config.device_mappings {
            if let Some(handle) = device_handles.get(&device_mapping.device_id) {
                devices.insert(device_mapping.device_id.clone(), handle.clone());
            } else {
                tracing::warn!("Device '{}' not found in plant - skipping", device_mapping.device_id);
            }
        }

        let config = plc_config.clone();
        tracing::info!("Spawning OPC UA server for {} on port {}", config.name, config.port);
        let plc_task = tokio::spawn(async move {
            if let Err(e) = opcua_server::plc_server::start_plc_server(config.clone(), devices).await {
                tracing::error!("{} server error: {}", config.name, e);
            }
        });

        plc_tasks.push(plc_task);
    }

    // Wait for PLC servers to start - they run in std::thread so need time to initialize
    tracing::info!("Waiting 10s for {} OPC UA server(s) to initialize...", plc_tasks.len());
    tokio::time::sleep(Duration::from_secs(10)).await;
    tracing::info!("OPC UA servers ready");

    // Start SCADA client (connects to all PLCs and aggregates data)
    let configs_for_scada = plant_config.plcs.clone();
    if let Err(e) = opcua_server::start_scada_client(tx_scada, configs_for_scada).await {
        tracing::error!("SCADA client error: {}", e);
    }
    tracing::info!("SCADA client started");

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
