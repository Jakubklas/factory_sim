use std::sync::Arc;
use tokio::sync::RwLock;
use opcua::server::{
    ServerBuilder,
    node_manager::memory::{simple_node_manager, SimpleNodeManager},
    address_space::Variable,
};
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::types::{DataValue, NodeId, Variant};
use plant_config::{DataType, PlcConfig, ResolvedDevice};
use crate::state::SimulatorState;

pub async fn start_one(
    state:   Arc<RwLock<SimulatorState>>,
    devices: Arc<Vec<ResolvedDevice>>,
    plc:     PlcConfig,
    tick_ms: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting OPC-UA server '{}' on port {}", plc.name, plc.port);

    let pki_dir = pki_dir_for_server(&plc.name);
    let namespace_uri = format!("urn:factory-sim:{}", plc.plc_id);
    let advertise_host = std::env::var("OPCUA_HOST")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());

    let node_mgr_builder = simple_node_manager(
        NamespaceMetadata { namespace_uri: namespace_uri.clone(), ..Default::default() },
        "factory-sim",
    );

    let (server, handle) = ServerBuilder::new_anonymous(&plc.name)
        .application_uri(format!("urn:factory-sim-server:{}", plc.plc_id))
        .host("0.0.0.0")
        .port(plc.port)
        .pki_dir(pki_dir)
        .create_sample_keypair(true)
        .trust_client_certs(true)
        .discovery_urls(vec![format!("opc.tcp://{}:{}/", advertise_host, plc.port)])
        .with_node_manager(node_mgr_builder)
        .build()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let node_mgr = handle.node_managers()
        .get_of_type::<SimpleNodeManager>()
        .ok_or("SimpleNodeManager not found in built server")?;

    // Namespace index for our nodes — determined by registration order.
    // With the default "server" feature (core + diagnostics pre-registered), our
    // namespace lands at index 2, matching the "ns=2;s=..." format produced by
    // endpoint_configs() and expected by the backend in Phase 0.
    let ns = handle.get_namespace_index(&namespace_uri).unwrap_or(2);

    // Collect node specs from device type definitions
    let node_specs = collect_node_specs(&devices, &plc, ns);

    // Populate address space
    {
        let mut as_ = node_mgr.address_space().write();

        let plc_folder_id = NodeId::new(ns, format!("folder/{}", plc.name));
        as_.add_folder(&plc_folder_id, &plc.name, &plc.name, &NodeId::objects_folder_id());

        for spec in &node_specs {
            let device_folder_id = NodeId::new(ns, format!("folder/{}/{}", plc.name, spec.device_id));
            as_.add_folder(&device_folder_id, &spec.device_id, &spec.device_id, &plc_folder_id);

            let node_id = NodeId::new(ns, spec.node_path.clone());
            let var = match &spec.initial_value {
                DataType::Float(f)   => Variable::new(&node_id, &spec.metric_name, &spec.metric_name, *f),
                DataType::Str(s)     => Variable::new(&node_id, &spec.metric_name, &spec.metric_name, s.as_str()),
                DataType::Boolean(b) => Variable::new(&node_id, &spec.metric_name, &spec.metric_name, *b),
            };
            as_.add_variables(vec![var], &device_folder_id);
        }
    }

    // Address-space update loop: reads SimulatorState snapshot, pushes to OPC-UA nodes.
    let subs       = handle.subscriptions().clone();
    let node_mgr2  = node_mgr.clone();
    let node_specs2 = node_specs.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
        loop {
            interval.tick().await;
            let snapshot = state.read().await.snapshot();
            for spec in &node_specs2 {
                let value = snapshot
                    .get(&spec.device_id)
                    .and_then(|fields| fields.get(&spec.metric_name));
                if let Some(data_type) = value {
                    let node_id = NodeId::new(spec.ns, spec.node_path.clone());
                    let variant  = datatype_to_variant(data_type);
                    let _ = node_mgr2.set_value(&subs, &node_id, None, DataValue::new_now(variant));
                }
            }
        }
    });

    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("OPC-UA server '{}' stopped: {}", plc.name, e);
        }
    });

    Ok(())
}

#[derive(Clone)]
struct NodeSpec {
    ns:            u16,
    device_id:     String,
    metric_name:   String,
    node_path:     String,
    initial_value: DataType,
}

fn collect_node_specs(devices: &[ResolvedDevice], plc: &PlcConfig, ns: u16) -> Vec<NodeSpec> {
    let plc_device_ids: Vec<&str> = plc.devices.iter()
        .map(|d| d.device_id.as_str())
        .collect();

    devices.iter()
        .filter(|d| plc_device_ids.contains(&d.config.device_id.as_str()))
        .flat_map(|d| {
            d.type_def.metrics.iter().map(move |m| NodeSpec {
                ns,
                device_id:     d.config.device_id.clone(),
                metric_name:   m.name.clone(),
                node_path:     format!("{}.{}.{}", plc.name, d.config.device_id, m.name),
                initial_value: m.initial_value.clone().unwrap_or(DataType::Float(0.0)),
            })
        })
        .collect()
}

fn pki_dir_for_server(plc_name: &str) -> std::path::PathBuf {
    let base = std::env::var("PKI_DIR").unwrap_or_else(|_| "./pki".to_string());
    let safe = plc_name.to_lowercase().replace(' ', "-");
    std::path::PathBuf::from(base).join("servers").join(safe)
}

fn datatype_to_variant(value: &DataType) -> Variant {
    match value {
        DataType::Float(f)   => Variant::Double(*f),
        DataType::Str(s)     => Variant::String(s.clone().into()),
        DataType::Boolean(b) => Variant::Boolean(*b),
    }
}
