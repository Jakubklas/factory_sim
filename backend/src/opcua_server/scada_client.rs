use opcua::client::prelude::*;
use tokio::sync::broadcast;
use std::sync::Arc;
use opcua::sync::RwLock;
use std::str::FromStr;
use crate::simulator::plant::{PlantState, DeviceState};
use crate::simulator::devices::{Boiler, BoilerStatus, PressureMeter, MeterStatus, FlowMeter, FlowMeterStatus, Valve, ValveMode, ValveStatus};
use std::collections::HashMap;
use crate::models::PlcConfig;

pub async fn start_scada_client(
    tx: broadcast::Sender<PlantState>,
    configs: Vec<PlcConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting SCADA Client - connecting to {} PLCs", configs.len());

    // Spawn SCADA client in a separate thread to avoid runtime conflicts
    std::thread::spawn(move || {
        run_scada_client_sync(tx, configs);
    });

    Ok(())
}

fn run_scada_client_sync(
    tx: broadcast::Sender<PlantState>,
    configs: Vec<PlcConfig>,
) {
    // Create clients and sessions dynamically for each PLC
    let mut sessions = Vec::new();
    let mut _clients = Vec::new(); // Keep clients alive to avoid dropping runtime

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
                _clients.push(client); // Keep client alive
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

    // Polling loop to read values from all PLCs (synchronous loop in this thread)
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut all_devices = HashMap::new();

        // Read from each PLC
        for (config, session) in &sessions {
            if let Ok(devices) = read_plc_data_sync(session, config) {
                all_devices.extend(devices);
            }
        }

        // Send aggregated data
        if !all_devices.is_empty() {
            let plant_state = PlantState { devices: all_devices };
            let _ = tx.send(plant_state);
        }
    }
}

fn read_plc_data_sync(
    session: &Arc<RwLock<Session>>,
    config: &PlcConfig,
) -> Result<HashMap<String, DeviceState>, Box<dyn std::error::Error>> {
    let session = session.read();
    let mut devices = HashMap::new();

    for device_mapping in &config.device_mappings {
        // Read all metrics for this device
        let mut metric_values: HashMap<String, serde_json::Value> = HashMap::new();

        for metric in &device_mapping.metrics {
            let node_path = format!("ns=2;s={}.{}", config.name, metric.node_path);

            match &metric.data_type {
                crate::models::DataType::Double => {
                    if let Ok(value) = read_node_value(&session, &node_path) {
                        metric_values.insert(metric.field_name.clone(), serde_json::json!(value));
                    }
                }
                crate::models::DataType::String => {
                    if let Ok(value) = read_node_string(&session, &node_path) {
                        metric_values.insert(metric.field_name.clone(), serde_json::json!(value));
                    }
                }
            }
        }

        // Construct DeviceState based on device type
        let device_state = match device_mapping.device_type.as_str() {
            "Boiler" => {
                let boiler = Boiler {
                    id: device_mapping.device_id.clone(),
                    temperature: metric_values.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    target_temperature: metric_values.get("target_temperature").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    pressure: metric_values.get("pressure").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    status: metric_values.get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| parse_boiler_status(s))
                        .unwrap_or(BoilerStatus::Off),
                };
                DeviceState::Boiler(boiler)
            }
            "PressureMeter" => {
                let meter = PressureMeter {
                    id: device_mapping.device_id.clone(),
                    pressure: metric_values.get("pressure").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    status: metric_values.get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| parse_meter_status(s))
                        .unwrap_or(MeterStatus::Normal),
                };
                DeviceState::PressureMeter(meter)
            }
            "FlowMeter" => {
                let meter = FlowMeter {
                    id: device_mapping.device_id.clone(),
                    flow_rate: metric_values.get("flow_rate").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    total_volume: metric_values.get("total_volume").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    status: metric_values.get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| parse_flow_meter_status(s))
                        .unwrap_or(FlowMeterStatus::Normal),
                };
                DeviceState::FlowMeter(meter)
            }
            "Valve" => {
                let valve = Valve {
                    id: device_mapping.device_id.clone(),
                    position: metric_values.get("position").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    mode: metric_values.get("mode")
                        .and_then(|v| v.as_str())
                        .map(|s| parse_valve_mode(s))
                        .unwrap_or(ValveMode::Auto),
                    status: metric_values.get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| parse_valve_status(s))
                        .unwrap_or(ValveStatus::Partial),
                };
                DeviceState::Valve(valve)
            }
            unknown => {
                tracing::warn!("Unknown device type: {}", unknown);
                continue;
            }
        };

        devices.insert(device_mapping.device_id.clone(), device_state);
    }

    Ok(devices)
}

fn read_node_value(session: &Session, node_id: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let node_id = NodeId::from_str(node_id)?;
    let read_result = session.read(&[ReadValueId::from(node_id)], TimestampsToReturn::Neither, 0.0)?;

    if let Some(data_value) = read_result.first() {
        if let Some(value) = &data_value.value {
            if let Variant::Double(val) = value {
                return Ok(*val);
            }
        }
    }

    Err("Failed to read value".into())
}

fn read_node_string(session: &Session, node_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let node_id = NodeId::from_str(node_id)?;
    let read_result = session.read(&[ReadValueId::from(node_id)], TimestampsToReturn::Neither, 0.0)?;

    if let Some(data_value) = read_result.first() {
        if let Some(value) = &data_value.value {
            if let Variant::String(val) = value {
                return Ok(val.as_ref().to_string());
            }
        }
    }

    Err("Failed to read string value".into())
}

fn parse_boiler_status(status_str: &str) -> BoilerStatus {
    match status_str {
        "Off" => BoilerStatus::Off,
        "Heating" => BoilerStatus::Heating,
        "Steady" => BoilerStatus::Steady,
        "Overheat" => BoilerStatus::Overheat,
        _ => BoilerStatus::Off,
    }
}

fn parse_meter_status(status_str: &str) -> MeterStatus {
    match status_str {
        "Normal" => MeterStatus::Normal,
        "Warning" => MeterStatus::Warning,
        "Critical" => MeterStatus::Critical,
        _ => MeterStatus::Normal,
    }
}

fn parse_flow_meter_status(status_str: &str) -> FlowMeterStatus {
    match status_str {
        "Normal" => FlowMeterStatus::Normal,
        "Low" => FlowMeterStatus::Low,
        "High" => FlowMeterStatus::High,
        _ => FlowMeterStatus::Normal,
    }
}

fn parse_valve_mode(mode_str: &str) -> ValveMode {
    match mode_str {
        "Manual" => ValveMode::Manual,
        "Auto" => ValveMode::Auto,
        _ => ValveMode::Auto,
    }
}

fn parse_valve_status(status_str: &str) -> ValveStatus {
    match status_str {
        "Open" => ValveStatus::Open,
        "Closed" => ValveStatus::Closed,
        "Partial" => ValveStatus::Partial,
        "Fault" => ValveStatus::Fault,
        _ => ValveStatus::Partial,
    }
}
