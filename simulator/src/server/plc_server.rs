use std::sync::Arc;
use tokio::sync::RwLock;
use opcua::server::{
    ServerBuilder,
    node_manager::memory::{simple_node_manager, SimpleNodeManager},
    address_space::{Variable, VariableBuilder},
};
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::types::{DataTypeId, DataValue, NodeId, NumericRange, StatusCode, Variant};
use plant_config::{DataType, PlcConfig, ResolvedDevice};
use crate::state::SimulatorState;
use crate::stats::SimStats;

pub async fn start_one(
    state:          Arc<RwLock<SimulatorState>>,
    devices:        Arc<Vec<ResolvedDevice>>,
    plc:            PlcConfig,
    tick_ms:        u64,
    advertise_host: String,
    pki_base:       String,
    stats:          Arc<SimStats>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting OPC-UA server '{}' on port {}", plc.name, plc.port);

    let pki_dir = pki_dir_for_server(&plc.name, &pki_base);
    let namespace_uri = format!("urn:factory-sim:{}", plc.plc_id);

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

    // Collect output metric nodes and writable input port nodes.
    let node_specs  = collect_node_specs(&devices, &plc, ns);
    let input_specs = collect_input_specs(&devices, &plc, ns);

    // Populate address space
    {
        let mut as_ = node_mgr.address_space().write();

        let plc_folder_id = NodeId::new(ns, format!("folder/{}", plc.name));
        as_.add_folder(&plc_folder_id, &plc.name, &plc.name, &NodeId::objects_folder_id());

        // One folder per device — created exactly once (a device may have both output
        // metrics and input ports; re-adding the same folder errors in this opcua version).
        let mut device_ids: Vec<&str> = node_specs.iter().map(|s| s.device_id.as_str())
            .chain(input_specs.iter().map(|s| s.device_id.as_str()))
            .collect();
        device_ids.sort();
        device_ids.dedup();
        for did in &device_ids {
            let device_folder_id = NodeId::new(ns, format!("folder/{}/{}", plc.name, did));
            as_.add_folder(&device_folder_id, *did, *did, &plc_folder_id);
        }

        // Output metric nodes (read-only). Variable::new infers the DataType from the value.
        for spec in &node_specs {
            let device_folder_id = NodeId::new(ns, format!("folder/{}/{}", plc.name, spec.device_id));
            let node_id = NodeId::new(ns, spec.node_path.clone());
            let var = match &spec.initial_value {
                DataType::Float(f)   => Variable::new(&node_id, &spec.metric_name, &spec.metric_name, *f),
                DataType::Str(s)     => Variable::new(&node_id, &spec.metric_name, &spec.metric_name, s.as_str()),
                DataType::Boolean(b) => Variable::new(&node_id, &spec.metric_name, &spec.metric_name, *b),
            };
            as_.add_variables(vec![var], &device_folder_id);
        }

        // Writable input port nodes — the backend writes these each tick to deliver wired values.
        // Node path: "{plc_name}.{device_id}.{input_name}" (same namespace as output metrics).
        // The explicit .data_type() is required: VariableBuilder::build() panics on a node with
        // a null DataType, and .value() alone doesn't set it (unlike Variable::new above).
        for ispec in &input_specs {
            let device_folder_id = NodeId::new(ns, format!("folder/{}/{}", plc.name, ispec.device_id));
            let node_id = NodeId::new(ns, ispec.node_path.clone());
            let var = VariableBuilder::new(&node_id, &ispec.input_name, &ispec.input_name)
                .data_type(DataTypeId::Double)
                .value(0.0_f64)
                .writable()
                .build();
            as_.add_variables(vec![var], &device_folder_id);
        }
    }

    // Register write callbacks for every input port node.
    // When the backend writes a value via OPC-UA, we update SimulatorState so the next
    // Jacobi tick picks it up from the snapshot.
    for ispec in &input_specs {
        let node_id   = NodeId::new(ns, ispec.node_path.clone());
        let state_cb  = Arc::clone(&state);
        let stats_cb  = Arc::clone(&stats);
        let device_id = ispec.device_id.clone();
        let input_nm  = ispec.input_name.clone();

        node_mgr.inner().add_write_callback(node_id, move |dv: DataValue, _range: &NumericRange| {
            let Some(variant) = dv.value else { return StatusCode::BadInvalidArgument };
            let data_type = match variant {
                Variant::Double(f)  => DataType::Float(f),
                Variant::Float(f)   => DataType::Float(f as f64),
                Variant::Boolean(b) => DataType::Boolean(b),
                Variant::String(s)  => DataType::Str(s.into()),
                _                   => return StatusCode::BadTypeMismatch,
            };

            stats_cb.record_write();

            let state_clone  = Arc::clone(&state_cb);
            let device_clone = device_id.clone();
            let field_clone  = input_nm.clone();

            tokio::spawn(async move {
                let mut s = state_clone.write().await;
                let mut fields = s.snapshot()
                    .get(&device_clone)
                    .cloned()
                    .unwrap_or_default();
                fields.insert(field_clone, data_type);
                s.set_device_state(&device_clone, fields);
            });

            StatusCode::Good
        });
    }

    // Address-space update loop: reads SimulatorState snapshot, pushes output metrics to OPC-UA nodes.
    let subs        = handle.subscriptions().clone();
    let node_mgr2   = node_mgr.clone();
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

#[derive(Clone)]
struct InputSpec {
    #[allow(dead_code)]
    ns:         u16,
    device_id:  String,
    input_name: String,
    node_path:  String,
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

fn collect_input_specs(devices: &[ResolvedDevice], plc: &PlcConfig, ns: u16) -> Vec<InputSpec> {
    let plc_device_ids: Vec<&str> = plc.devices.iter()
        .map(|d| d.device_id.as_str())
        .collect();

    devices.iter()
        .filter(|d| plc_device_ids.contains(&d.config.device_id.as_str()))
        .flat_map(|d| {
            d.config.input_variables.iter().map(move |iv| InputSpec {
                ns,
                device_id:  d.config.device_id.clone(),
                input_name: iv.name.clone(),
                // Same node path pattern as outputs — input ports live in the same device subtree.
                node_path:  format!("{}.{}.{}", plc.name, d.config.device_id, iv.name),
            })
        })
        .collect()
}

fn pki_dir_for_server(plc_name: &str, base: &str) -> std::path::PathBuf {
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
