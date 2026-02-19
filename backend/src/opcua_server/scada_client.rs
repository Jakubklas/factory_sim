use opcua::client::prelude::*;
use tokio::sync::broadcast;
use std::sync::Arc;
use opcua::sync::RwLock;
use std::str::FromStr;
use crate::simulator::plant::PlantState;
use crate::models::{PlcConfig, DeviceFieldValue, DataType};
use std::collections::HashMap;

pub async fn start_scada_client(
    tx: broadcast::Sender<PlantState>,
    configs: Vec<PlcConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting SCADA Client - connecting to {} PLCs", configs.len());

    std::thread::spawn(move || {
        run_scada_client_sync(tx, configs);
    });

    Ok(())
}

fn run_scada_client_sync(
    tx: broadcast::Sender<PlantState>,
    configs: Vec<PlcConfig>,
) {
    let mut sessions = Vec::new();
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

        match client.connect_to_endpoint(
            (
                config.endpoint.as_str(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
                UserTokenPolicy::anonymous(),
            ),
            IdentityToken::Anonymous,
        ) {
            Ok(session) => {
                sessions.push((config.clone(), session));
                _clients.push(client);
                tracing::info!("SCADA connected to {}", config.name);
            }
            Err(e) => {
                tracing::error!("Failed to connect SCADA to {}: {}", config.name, e);
                continue;
            }
        }
    }

    if sessions.is_empty() {
        tracing::warn!("SCADA client could not connect to any PLCs - will not publish data");
        return;
    }

    tracing::info!("SCADA client successfully connected to {} PLC(s)", sessions.len());

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut all_devices: HashMap<String, HashMap<String, DeviceFieldValue>> = HashMap::new();

        for (config, session) in &sessions {
            if let Ok(devices) = read_plc_data(session, config) {
                all_devices.extend(devices);
            }
        }

        if !all_devices.is_empty() {
            let _ = tx.send(PlantState { devices: all_devices });
        }
    }
}

/// Read all device fields from a PLC generically - no hardcoded device types
fn read_plc_data(
    session: &Arc<RwLock<Session>>,
    config: &PlcConfig,
) -> Result<HashMap<String, HashMap<String, DeviceFieldValue>>, Box<dyn std::error::Error>> {
    let session = session.read();
    let mut devices = HashMap::new();

    for device_mapping in &config.device_mappings {
        let mut fields = HashMap::new();

        for metric in &device_mapping.metrics {
            let node_path = format!("ns=2;s={}.{}", config.name, metric.node_path);

            let value = match &metric.data_type {
                DataType::Double => {
                    read_node_f64(&session, &node_path)
                        .ok()
                        .map(DeviceFieldValue::Float)
                }
                DataType::String => {
                    read_node_string(&session, &node_path)
                        .ok()
                        .map(DeviceFieldValue::String)
                }
            };

            if let Some(v) = value {
                fields.insert(metric.field_name.clone(), v);
            }
        }

        devices.insert(device_mapping.device_id.clone(), fields);
    }

    Ok(devices)
}

fn read_node_f64(session: &Session, node_id: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let node_id = NodeId::from_str(node_id)?;
    let result = session.read(&[ReadValueId::from(node_id)], TimestampsToReturn::Neither, 0.0)?;

    if let Some(data_value) = result.first() {
        if let Some(Variant::Double(val)) = &data_value.value {
            return Ok(*val);
        }
    }
    Err("Failed to read f64 value".into())
}

fn read_node_string(session: &Session, node_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let node_id = NodeId::from_str(node_id)?;
    let result = session.read(&[ReadValueId::from(node_id)], TimestampsToReturn::Neither, 0.0)?;

    if let Some(data_value) = result.first() {
        if let Some(Variant::String(val)) = &data_value.value {
            return Ok(val.as_ref().to_string());
        }
    }
    Err("Failed to read string value".into())
}
