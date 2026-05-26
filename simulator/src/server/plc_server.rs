use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use opcua::server::prelude::*;
use opcua::server::config::{ServerConfig, TcpConfig};
use opcua::server::server::Server;
use plant_config::{DataType, PlcConfig, ResolvedDevice};
use crate::state::SimulatorState;

pub async fn start_one(
    state:   Arc<RwLock<SimulatorState>>,
    devices: Arc<Vec<ResolvedDevice>>,
    plc:     PlcConfig,
    tick_ms: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting OPC-UA server '{}' on port {}", plc.name, plc.port);

    let node_specs = collect_node_specs(&devices, &plc);

    let mut server_config = ServerConfig::default();
    server_config.application_name       = plc.name.clone();
    server_config.application_uri        = format!("urn:factory-sim:{}", plc.plc_id);
    server_config.product_uri            = format!("urn:factory-sim:{}", plc.plc_id);
    server_config.create_sample_keypair  = true;
    server_config.pki_dir                = pki_dir_for_server(&plc.name);
    server_config.discovery_server_url   = None;
    // Bind to 0.0.0.0 so other containers/hosts on the network can reach us.
    server_config.tcp_config = TcpConfig {
        hello_timeout: 10,
        host:          "0.0.0.0".to_string(),
        port:          plc.port,
    };
    // Advertise via the hostname clients should connect on. Resolution order:
    // OPCUA_HOST (explicit) → HOSTNAME (Docker sets this to the service/container name) → localhost.
    let advertise_host = std::env::var("OPCUA_HOST")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());
    server_config.discovery_urls = vec![format!("opc.tcp://{}:{}", advertise_host, plc.port)];
    server_config.endpoints.insert(
        "none".to_string(),
        ServerEndpoint::new_none("/", &[ANONYMOUS_USER_TOKEN_ID.to_string()]),
    );

    let server = Server::new(server_config);

    // Build address space: PLC folder → device folders → metric variables
    {
        let address_space = server.address_space();
        let mut as_ = address_space.write();

        let plc_folder = as_
            .add_folder(&plc.name, &plc.name, &NodeId::objects_folder_id())
            .expect("Failed to create PLC folder");

        let mut seen: HashSet<&str> = HashSet::new();
        let device_ids: Vec<&str> = node_specs
            .iter()
            .filter(|s| seen.insert(s.device_id.as_str()))
            .map(|s| s.device_id.as_str())
            .collect();

        for device_id in &device_ids {
            let device_folder = as_
                .add_folder(*device_id, *device_id, &plc_folder)
                .expect("Failed to create device folder");

            let variables: Vec<Variable> = node_specs
                .iter()
                .filter(|s| s.device_id.as_str() == *device_id)
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

    // Address-space update loop: reads SimulatorState snapshot, pushes to OPC-UA nodes
    let address_space = server.address_space();
    let plc_name      = plc.name.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
        loop {
            interval.tick().await;
            let snapshot = state.read().await.snapshot();
            let mut as_  = address_space.write();
            for spec in &node_specs {
                let value = snapshot
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

    let plc_name_thread = plc_name.clone();
    std::thread::Builder::new()
        .name(format!("{}-opcua", plc_name))
        .spawn(move || {
            tracing::info!("OPC-UA server thread started: {}", plc_name_thread);
            server.run();
            tracing::info!("OPC-UA server thread stopped: {}", plc_name_thread);
        })
        .expect("Failed to spawn OPC-UA server thread");

    Ok(())
}

struct NodeSpec {
    device_id:     String,
    metric_name:   String,
    node_path:     String,
    initial_value: DataType,
}

fn collect_node_specs(devices: &[ResolvedDevice], plc: &PlcConfig) -> Vec<NodeSpec> {
    let plc_device_ids: Vec<&str> = plc.devices.iter()
        .map(|d| d.device_id.as_str())
        .collect();

    devices.iter()
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

/// PKI directory for the OPC-UA server's auto-generated keypair.
/// Read from `PKI_DIR` (defaults to `./pki`) and namespaced per PLC so multiple simulators
/// on the same host don't clobber each other's certs.
fn pki_dir_for_server(plc_name: &str) -> std::path::PathBuf {
    let base = std::env::var("PKI_DIR").unwrap_or_else(|_| "./pki".to_string());
    let safe = plc_name.to_lowercase().replace(' ', "-");
    std::path::PathBuf::from(base).join("servers").join(safe)
}

fn datatype_to_variant(value: &DataType) -> Variant {
    match value {
        DataType::Float(f)   => Variant::Double(*f),
        DataType::Str(s)     => Variant::String(UAString::from(s.as_str())),
        DataType::Boolean(b) => Variant::Boolean(*b),
    }
}
