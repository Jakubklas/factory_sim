use opcua::server::prelude::*;
use opcua::server::config::{TcpConfig, ServerConfig};
use opcua::server::server::Server;
use std::collections::HashMap;
use crate::models::{PlcConfig, DeviceMapping, DataType, DeviceHandle, DeviceFieldValue};

pub async fn start_plc_server(
    config: PlcConfig,
    devices: HashMap<String, DeviceHandle>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting {} OPC UA Server on port {}", config.name, config.port);

    // Create a ServerConfig first and modify its tcp_config
    let mut server_config = ServerConfig::default();
    server_config.application_name = config.name.clone();
    server_config.application_uri = config.uri.clone();
    server_config.create_sample_keypair = false; // TODO: Re-enable for production
    server_config.pki_dir = format!("./pki-{}", config.name.to_lowercase().replace(" ", "-")).into();
    server_config.discovery_server_url = None;

    // Set TCP config with the port from the JSON configuration
    server_config.tcp_config = TcpConfig {
        hello_timeout: 10,
        host: "0.0.0.0".to_string(),
        port: config.port,
    };

    // Add discovery URLs - use the constructed endpoint from host:port
    let discovery_url = format!("opc.tcp://{}:{}", server_config.tcp_config.host, server_config.tcp_config.port);
    server_config.discovery_urls = vec![discovery_url];

    // Add endpoint - ServerEndpoint::new_none expects a path (like "/"), not a full URL
    server_config.endpoints.insert(
        "none".to_string(),
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()])
    );

    let server = Server::new(server_config);

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
                // Include PLC name in node ID to match SCADA client expectations
                let node_id = NodeId::new(2, format!("{}.{}", config.name, metric.node_path));
                let variable: Variable = match metric.data_type {
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
    let plc_name = config.name.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));

        loop {
            interval.tick().await;

            let mut address_space = address_space.write();

            // Update values for each device
            for device_mapping in &device_mappings {
                if let Some(device_handle) = devices.get(&device_mapping.device_id) {
                    update_device_values(&mut address_space, device_handle, device_mapping, &plc_name).await;
                }
            }
        }
    });

    // Run the opcua server listening to the simulator updates
    // OPC UA server.run() creates its own runtime, so spawn it in a std::thread
    let name = config.name.clone();
    std::thread::Builder::new()
        .name(format!("{}-server", name))
        .spawn(move || {
            tracing::info!("{} OPC UA server thread started", name);
            server.run();
            tracing::info!("{} OPC UA server stopped", name);
        })
        .expect("Failed to spawn OPC UA server thread");

    tracing::info!("{} OPC UA server spawned successfully", config.name);

    // Return immediately - server runs in background thread
    Ok(())
}


// Fully generic update function - all device-specific logic is now in the models layer
async fn update_device_values(
    address_space: &mut AddressSpace,
    device_handle: &DeviceHandle,
    device_mapping: &DeviceMapping,
    plc_name: &str,
) {
    for metric in &device_mapping.metrics {
        // Use the generic read_field method from DeviceHandle
        if let Some(field_value) = device_handle.read_field(&metric.field_name).await {
            // Match SCADA client's expected node path format
            let node_id = NodeId::new(2, format!("{}.{}", plc_name, metric.node_path));

            // Convert DeviceFieldValue to OPC UA Variant
            let value: Variant = match field_value {
                DeviceFieldValue::Float(v) => v.into(),
                DeviceFieldValue::String(s) => UAString::from(s).into(),
            };

            let _ = address_space.set_variable_value(node_id, value, &DateTime::now(), &DateTime::now());
        }
    }
}
