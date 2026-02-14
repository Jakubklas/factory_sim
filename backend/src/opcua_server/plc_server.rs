use opcua::server::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use crate::simulator::devices::{Boiler, PressureMeter, FlowMeter, Valve};
use crate::models::{PlcConfig, DeviceMapping, DeviceType, MetricConfig, DataType};

pub enum DeviceHandle {
    Boiler(Arc<RwLock<Boiler>>),
    PressureMeter(Arc<RwLock<PressureMeter>>),
    FlowMeter(Arc<RwLock<FlowMeter>>),
    Valve(Arc<RwLock<Valve>>),
}

pub async fn start_plc_server(
    config: PlcConfig,
    devices: HashMap<String, DeviceHandle>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting {} OPC UA Server on port {}", config.name, config.port);

    let server = ServerBuilder::new()
        .application_name(&config.name)
        .application_uri(&config.uri)
        .discovery_urls(vec![config.endpoint.clone()])
        .create_sample_keypair(true)
        .pki_dir(format!("./pki-{}", config.name.to_lowercase().replace(" ", "-")))
        .discovery_server_url(None)
        .host_and_port("0.0.0.0", config.port)
        .server()
        .expect("Failed to create OPC UA server");

    // Create address space dynamically from config
    {
        let address_space = server.address_space();
        let mut address_space = address_space.write();

        // Create PLC folder
        let plc_folder = address_space
            .add_folder(&config.name, &config.name, &NodeId::objects_folder_id())
            .expect("Failed to create PLC folder");

        // Create folders and variables for each device
        for device_mapping in &config.device_mappings {
            let device_folder = address_space
                .add_folder(&device_mapping.folder_name, &device_mapping.folder_name, &plc_folder)
                .expect("Failed to create device folder");

            // Create variables for each metric
            let mut variables = Vec::new();
            for metric in &device_mapping.metrics {
                let node_id = NodeId::new(2, format!("{}.{}", device_mapping.folder_name, &metric.field_name));
                let variable = match metric.data_type {
                    DataType::Double => Variable::new(&node_id, &metric.field_name, &metric.field_name, 0.0_f64),
                    DataType::String => Variable::new(&node_id, &metric.field_name, &metric.field_name, UAString::from("")),
                };
                variables.push(variable);
            }
            address_space.add_variables(variables, &device_folder);
        }
    }

    // Spawn update task
    let address_space = server.address_space();
    let device_mappings = config.device_mappings.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));

        loop {
            interval.tick().await;

            let mut address_space = address_space.write();

            // Update values for each device
            for device_mapping in &device_mappings {
                if let Some(device_handle) = devices.get(&device_mapping.device_id) {
                    update_device_values(&mut address_space, device_handle, device_mapping).await;
                }
            }
        }
    });

    server.run();
    Ok(())
}

async fn update_device_values(
    address_space: &mut AddressSpace,
    device_handle: &DeviceHandle,
    device_mapping: &DeviceMapping,
) {
    match device_handle {
        DeviceHandle::Boiler(boiler) => {
            let data = boiler.read().await;
            for metric in &device_mapping.metrics {
                let node_id = NodeId::new(2, format!("{}.{}", device_mapping.folder_name, &metric.field_name));
                let value: Variant = match metric.field_name.as_str() {
                    "temperature" => data.temperature.into(),
                    "target_temperature" => data.target_temperature.into(),
                    "pressure" => data.pressure.into(),
                    "status" => UAString::from(format!("{:?}", data.status)).into(),
                    _ => continue,
                };
                let _ = address_space.set_variable_value(node_id, value, &DateTime::now(), &DateTime::now());
            }
        }
        DeviceHandle::PressureMeter(meter) => {
            let data = meter.read().await;
            for metric in &device_mapping.metrics {
                let node_id = NodeId::new(2, format!("{}.{}", device_mapping.folder_name, &metric.field_name));
                let value: Variant = match metric.field_name.as_str() {
                    "pressure" => data.pressure.into(),
                    "status" => UAString::from(format!("{:?}", data.status)).into(),
                    _ => continue,
                };
                let _ = address_space.set_variable_value(node_id, value, &DateTime::now(), &DateTime::now());
            }
        }
        DeviceHandle::FlowMeter(meter) => {
            let data = meter.read().await;
            for metric in &device_mapping.metrics {
                let node_id = NodeId::new(2, format!("{}.{}", device_mapping.folder_name, &metric.field_name));
                let value: Variant = match metric.field_name.as_str() {
                    "flow_rate" => data.flow_rate.into(),
                    "total_volume" => data.total_volume.into(),
                    "status" => UAString::from(format!("{:?}", data.status)).into(),
                    _ => continue,
                };
                let _ = address_space.set_variable_value(node_id, value, &DateTime::now(), &DateTime::now());
            }
        }
        DeviceHandle::Valve(valve) => {
            let data = valve.read().await;
            for metric in &device_mapping.metrics {
                let node_id = NodeId::new(2, format!("{}.{}", device_mapping.folder_name, &metric.field_name));
                let value: Variant = match metric.field_name.as_str() {
                    "position" => data.position.into(),
                    "mode" => UAString::from(format!("{:?}", data.mode)).into(),
                    "status" => UAString::from(format!("{:?}", data.status)).into(),
                    _ => continue,
                };
                let _ = address_space.set_variable_value(node_id, value, &DateTime::now(), &DateTime::now());
            }
        }
    }
}
