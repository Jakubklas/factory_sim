use opcua::client::prelude::*;
use tokio::sync::broadcast;
use std::sync::Arc;
use opcua::sync::RwLock;
use std::str::FromStr;
use std::collections::HashMap;
use crate::models::{DataType, DeviceMetric, PlantConfig, PlcConfig};
use crate::simulator::plant::PlantState;

// ============================================================================
// Entry point
// ============================================================================

/// Spawn the SCADA polling loop in a background thread.
/// Reads all PLC data on each tick and broadcasts the result via `tx`.
pub async fn start_scada_client(
    tx: broadcast::Sender<PlantState>,
    plant: &PlantConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let configs: Vec<PlcConfig> = plant.plcs.clone();
    let default_tick_ms = plant.default_tick_ms;

    tracing::info!("Starting SCADA client — connecting to {} PLC(s)", configs.len());

    std::thread::spawn(move || {
        run_scada_sync(tx, configs, default_tick_ms);
    });

    Ok(())
}

// ============================================================================
// Sync polling loop (runs in its own thread — OPC-UA SDK is sync)
// ============================================================================

fn run_scada_sync(
    tx: broadcast::Sender<PlantState>,
    configs: Vec<PlcConfig>,
    default_tick_ms: u64,
) {
    let mut sessions: Vec<(PlcConfig, Arc<RwLock<Session>>)> = Vec::new();
    let mut _clients = Vec::new();

    for config in &configs {
        let mut client = ClientBuilder::new()
            .application_name("SCADA")
            .application_uri("urn:SCADA")
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_limit(3)
            .client()
            .unwrap();

        let endpoint = format!("{}:{}{}", config.uri, config.port, config.endpoint);

        match client.connect_to_endpoint(
            (
                endpoint.as_str(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
                UserTokenPolicy::anonymous(),
            ),
            IdentityToken::Anonymous,
        ) {
            Ok(session) => {
                tracing::info!("SCADA connected to PLC '{}' at {}", config.name, endpoint);
                sessions.push((config.clone(), session));
                _clients.push(client);
            }
            Err(e) => {
                tracing::error!("Failed to connect to PLC '{}': {}", config.name, e);
            }
        }
    }

    if sessions.is_empty() {
        tracing::warn!("SCADA could not connect to any PLCs — exiting");
        return;
    }

    loop {
        std::thread::sleep(std::time::Duration::from_millis(default_tick_ms));

        let mut all_devices: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for (config, session) in &sessions {
            if let Ok(device_data) = read_plc_data(session, config) {
                all_devices.extend(device_data);
            }
        }

        if !all_devices.is_empty() {
            let _ = tx.send(PlantState { devices: all_devices });
        }
    }
}

// ============================================================================
// Individual OPC-UA reads
// ============================================================================

/// Read all metric fields for every device on a PLC.
/// Returns: device_id → field_name → value
fn read_plc_data(
    session: &Arc<RwLock<Session>>,
    config: &PlcConfig,
) -> Result<HashMap<String, HashMap<String, f64>>, Box<dyn std::error::Error>> {
    let session = session.read();
    let mut devices = HashMap::new();

    for device in &config.devices {
        let mut fields = HashMap::new();

        for metric in &device.metrics {
            // Node path convention: "PLC_name.device_id.metric_name"
            let node_id = format!("ns=2;s={}.{}.{}", config.name, device.device_id, metric.name);

            if let Ok(value) = read_node_f64(&session, &node_id) {
                fields.insert(metric.name.clone(), value);
            }
        }

        devices.insert(device.device_id.clone(), fields);
    }

    Ok(devices)
}

fn read_node_f64(session: &Session, node_id: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let node_id = NodeId::from_str(node_id)?;
    let result = session.read(
        &[ReadValueId::from(node_id)],
        TimestampsToReturn::Neither,
        0.0,
    )?;

    match result.first().and_then(|dv| dv.value.as_ref()) {
        Some(Variant::Double(v)) => Ok(*v),
        Some(Variant::Float(v))  => Ok(*v as f64),
        Some(Variant::Int32(v))  => Ok(*v as f64),
        _ => Err("Unexpected or missing value type".into()),
    }
}
