use std::sync::Arc;
use tokio::sync::RwLock;
use opcua::server::prelude::*;
use opcua::server::config::{ServerConfig, TcpConfig};
use opcua::server::server::Server;
use crate::config_handle::PlantConfigHandle;
use crate::models::{DataType, PlcConfig};

// ============================================================================
// Entry point — one OPC-UA server per PLC
// ============================================================================

/// Start one OPC-UA server per PLC in the plant config.
/// Servers bind at the addresses the config specifies — connector layer reads
/// those same addresses from PlantConfigHandle::endpoint_configs().
pub async fn start(
    handle: Arc<RwLock<PlantConfigHandle>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let plcs = handle.read().await.all_plcs().to_vec();

    for plc in plcs {
        start_plc_server(Arc::clone(&handle), plc).await?;
    }

    Ok(())
}

// ============================================================================
// Per-PLC server
// ============================================================================

async fn start_plc_server(
    handle: Arc<RwLock<PlantConfigHandle>>,
    plc:    PlcConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting OPC-UA server '{}' on port {}", plc.name, plc.port);

    let node_specs: Vec<NodeSpec> = {
        let h = handle.read().await;
        collect_node_specs(&h, &plc)
    };

    // -------------------------------------------------------------------------
    // Build OPC-UA server
    // -------------------------------------------------------------------------
    let mut server_config = ServerConfig::default();
    server_config.application_name = plc.name.clone();
    server_config.application_uri  = plc.uri.clone();
    server_config.create_sample_keypair = true;
    server_config.pki_dir = format!("./pki-{}", plc.name.to_lowercase().replace(' ', "-")).into();
    server_config.discovery_server_url = None;
    server_config.tcp_config = TcpConfig {
        hello_timeout: 10,
        host: "0.0.0.0".to_string(),
        port: plc.port,
    };
    server_config.discovery_urls = vec![
        format!("opc.tcp://0.0.0.0:{}", plc.port)
    ];
    server_config.endpoints.insert(
        "none".to_string(),
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
    );

    let server = Server::new(server_config);

    // -------------------------------------------------------------------------
    // Build address space: PLC folder → device folders → metric variables
    // -------------------------------------------------------------------------
    {
        let address_space = server.address_space();
        let mut as_ = address_space.write();

        let plc_folder = as_
            .add_folder(&plc.name, &plc.name, &NodeId::objects_folder_id())
            .expect("Failed to create PLC folder");

        let mut device_ids_seen: Vec<String> = Vec::new();
        for spec in &node_specs {
            if !device_ids_seen.contains(&spec.device_id) {
                device_ids_seen.push(spec.device_id.clone());
            }
        }

        for device_id in &device_ids_seen {
            let device_folder = as_
                .add_folder(device_id, device_id, &plc_folder)
                .expect("Failed to create device folder");

            let variables: Vec<Variable> = node_specs
                .iter()
                .filter(|s| &s.device_id == device_id)
                .map(|s| {
                    let node_id = NodeId::new(2, s.node_path.clone());
                    match &s.initial_value {
                        DataType::Float(f)   => Variable::new(&node_id, &s.metric_name, &s.metric_name, *f),
                        DataType::Str(str)   => Variable::new(&node_id, &s.metric_name, &s.metric_name, UAString::from(str.as_str())),
                        DataType::Boolean(b) => Variable::new(&node_id, &s.metric_name, &s.metric_name, *b),
                    }
                })
                .collect();

            as_.add_variables(variables, &device_folder);
        }
    }

    // -------------------------------------------------------------------------
    // Spawn address space update loop (reads LiveState → pushes to OPC-UA nodes)
    // -------------------------------------------------------------------------
    let address_space = server.address_space();
    let plc_name      = plc.name.clone();
    let tick_ms       = handle.read().await.default_tick_ms();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_millis(tick_ms)
        );
        loop {
            interval.tick().await;
            let state_snapshot = handle.read().await.state_snapshot();
            let mut as_ = address_space.write();
            for spec in &node_specs {
                let value = state_snapshot
                    .get(&spec.device_id)
                    .and_then(|fields| fields.get(&spec.metric_name));
                if let Some(data_type) = value {
                    let node_id = NodeId::new(2, spec.node_path.clone());
                    let _ = as_.set_variable_value(
                        node_id, datatype_to_variant(data_type),
                        &DateTime::now(), &DateTime::now(),
                    );
                }
            }
        }
    });

    // -------------------------------------------------------------------------
    // Run OPC-UA server in its own thread (server.run() is blocking)
    // -------------------------------------------------------------------------
    std::thread::Builder::new()
        .name(format!("{}-opcua", plc_name))
        .spawn(move || {
            tracing::info!("OPC-UA server thread started: {}", plc_name);
            server.run();
            tracing::info!("OPC-UA server thread stopped: {}", plc_name);
        })
        .expect("Failed to spawn OPC-UA server thread");

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

struct NodeSpec {
    device_id:     String,
    metric_name:   String,
    node_path:     String,   // "{plc_name}.{device_id}.{metric_name}"
    initial_value: DataType,
}

fn collect_node_specs(handle: &PlantConfigHandle, plc: &PlcConfig) -> Vec<NodeSpec> {
    let plc_device_ids: Vec<&str> = plc.devices.iter()
        .map(|d| d.device_id.as_str())
        .collect();

    handle.resolved_devices()
        .iter()
        .filter(|d| plc_device_ids.contains(&d.config.device_id.as_str()))
        .flat_map(|d| {
            d.type_def.metrics.iter().map(move |m| NodeSpec {
                device_id:     d.config.device_id.clone(),
                metric_name:   m.name.clone(),
                node_path:     format!("{}.{}.{}", plc.name, d.config.device_id, m.name),
                initial_value: m.initial_value.clone().unwrap_or(DataType::Float(0.0)),
            })
        })
        .collect()
}

fn datatype_to_variant(value: &DataType) -> Variant {
    match value {
        DataType::Float(f)   => Variant::Double(*f),
        DataType::Str(s)     => Variant::String(UAString::from(s.as_str())),
        DataType::Boolean(b) => Variant::Boolean(*b),
    }
}
