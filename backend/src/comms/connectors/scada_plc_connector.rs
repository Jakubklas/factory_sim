use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;
use opcua::client::{ClientBuilder, IdentityToken};
use opcua::types::{
    AttributeId, BrowseDescription, BrowseDirection, BrowseResultMask,
    DataValue, EndpointDescription, Identifier, MessageSecurityMode,
    NodeClass, NodeClassMask, NodeId, ObjectId, ReadValueId,
    ReferenceTypeId, TimestampsToReturn, UserTokenPolicy, Variant,
    WriteValue,
};
use plant_config::DataType;
use crate::comms::generic_connector::{BrowsedNode, ConnectorImpl, PartialState, WriteCmd};

// ============================================================================
// Connection handle
// ============================================================================

pub struct PlcConnection {
    session: Arc<opcua::client::Session>,
}

// ============================================================================
// Internal node read spec — rebuilt from browse results on every (re)connect.
// ============================================================================

#[derive(Clone)]
struct NodeRead {
    device_id:   String,
    metric_name: String,
    node_id:     NodeId,
    data_type:   NodeDataType,
}

#[derive(Clone)]
enum NodeDataType { Float, Str, Boolean }

// ============================================================================
// ScadaPlcConnector — one instance per OPC-UA endpoint
// ============================================================================

pub struct ScadaPlcConnector {
    endpoint:   String,
    plc_name:   String,
    pki_dir:    String,
    // Populated by browse(), consumed by poll(). Cleared on reconnect.
    node_reads: Arc<RwLock<Vec<NodeRead>>>,
}

impl ScadaPlcConnector {
    pub fn new(endpoint: String, plc_name: String, pki_dir: String) -> Self {
        Self { endpoint, plc_name, pki_dir, node_reads: Arc::new(RwLock::new(Vec::new())) }
    }

    pub fn from_endpoint_config(config: plant_config::PlcEndpointConfig, pki_dir: String) -> (String, Self) {
        let name = config.name.clone();
        let connector = Self::new(config.url, config.name, pki_dir);
        (name, connector)
    }
}

#[async_trait]
impl ConnectorImpl for ScadaPlcConnector {
    type Conn = PlcConnection;

    async fn connect(&self) -> Result<PlcConnection, Box<dyn std::error::Error + Send + Sync>> {
        let pki_dir = std::path::PathBuf::from(&self.pki_dir).join("clients").join("scada");

        let mut client = ClientBuilder::new()
            .application_name("factory-sim-scada")
            .application_uri("urn:factory-sim-scada")
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_limit(-1)
            .pki_dir(pki_dir)
            .client()
            .map_err(|e| format!("ClientBuilder::client() failed: {:?}", e))?;

        let endpoint: EndpointDescription = (
            self.endpoint.as_str(),
            "None",
            MessageSecurityMode::None,
            UserTokenPolicy::anonymous(),
        ).into();

        let (session, event_loop) = client
            .connect_to_matching_endpoint(endpoint, IdentityToken::Anonymous)
            .await
            .map_err(|e| format!("connect_to_matching_endpoint failed: {:?}", e))?;

        event_loop.spawn();
        session.wait_for_connection().await;

        // Clear stale node_reads from a previous connection so poll()
        // returns empty rather than stale results until browse() completes.
        self.node_reads.write().await.clear();

        Ok(PlcConnection { session })
    }

    async fn browse(&self, conn: &PlcConnection) -> Result<Vec<BrowsedNode>, Box<dyn std::error::Error + Send + Sync>> {
        let start: NodeId = ObjectId::ObjectsFolder.into();
        let raw = browse_tree(&conn.session, start)
            .await
            .map_err(|e| format!("[{}] browse failed: {:?}", self.plc_name, e))?;

        // Variable nodes are the only ones we can poll. Parse their NodeIds eagerly
        // so we can batch-read DataType attributes in one round-trip.
        let parseable: Vec<(&RawNode, NodeId)> = raw.iter()
            .filter(|n| n.is_variable)
            .filter_map(|n| NodeId::from_str(&n.node_id).ok().map(|id| (n, id)))
            .collect();

        let node_reads = if !parseable.is_empty() {
            let dt_reads: Vec<ReadValueId> = parseable.iter()
                .map(|(_, id)| ReadValueId {
                    node_id:      id.clone(),
                    attribute_id: AttributeId::DataType as u32,
                    ..Default::default()
                })
                .collect();

            let dt_results = conn.session
                .read(&dt_reads, TimestampsToReturn::Neither, 0.0)
                .await
                .map_err(|e| format!("[{}] DataType read failed: {:?}", self.plc_name, e))?;

            parseable.iter().zip(dt_results.iter())
                .filter_map(|((raw_node, node_id), dv)| {
                    let dt = dv.value.as_ref().map(data_type_from_variant).unwrap_or(NodeDataType::Float);
                    let (device_id, metric_name) = path_to_device_metric(&raw_node.browse_path)?;
                    Some(NodeRead { device_id, metric_name, node_id: node_id.clone(), data_type: dt })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        tracing::info!(
            "[{}] browse: {} total nodes, {} variable → {} pollable",
            self.plc_name, raw.len(), parseable.len(), node_reads.len()
        );

        *self.node_reads.write().await = node_reads;

        let browsed = raw.into_iter().map(|n| BrowsedNode {
            node_id:     n.node_id,
            browse_name: n.browse_name,
            browse_path: n.browse_path,
            datatype:    None,
            is_variable: n.is_variable,
            writable:    false,
        }).collect();

        Ok(browsed)
    }

    async fn write(&self, conn: &PlcConnection, cmd: &WriteCmd) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let node_id = NodeId::from_str(&cmd.node_id)
            .map_err(|e| format!("invalid node_id '{}': {:?}", cmd.node_id, e))?;

        let variant = datatype_to_variant(&cmd.value);

        let results = conn.session
            .write(&[WriteValue {
                node_id,
                attribute_id: AttributeId::Value as u32,
                value: DataValue::new_now(variant),
                ..Default::default()
            }])
            .await
            .map_err(|e| format!("[{}] session.write failed: {:?}", self.plc_name, e))?;

        if let Some(status) = results.into_iter().next() {
            if status.is_bad() {
                return Err(format!(
                    "[{}] write '{}' returned bad status: {:?}",
                    self.plc_name, cmd.node_id, status
                ).into());
            }
        }
        Ok(())
    }

    async fn poll(&self, conn: &PlcConnection) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>> {
        let node_reads = self.node_reads.read().await.clone();

        if node_reads.is_empty() {
            return Ok(PartialState::new());
        }

        let read_ids: Vec<ReadValueId> = node_reads.iter()
            .map(|n| ReadValueId {
                node_id:      n.node_id.clone(),
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            })
            .collect();

        let results = conn.session
            .read(&read_ids, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(|e| format!("[{}] session.read failed: {:?}", self.plc_name, e))?;

        let mut partial = PartialState::new();

        for (node, dv) in node_reads.iter().zip(results.iter()) {
            match extract_value(dv, &node.data_type) {
                Ok(value) => {
                    tracing::trace!("{}.{}.{} = {}", self.plc_name, node.device_id, node.metric_name, value);
                    partial
                        .entry(node.device_id.clone())
                        .or_default()
                        .insert(node.metric_name.clone(), value);
                }
                Err(e) => tracing::debug!(
                    "[{}] {}.{} read error (skipped): {}",
                    self.plc_name, node.device_id, node.metric_name, e
                ),
            }
        }

        Ok(partial)
    }
}

// ============================================================================
// Browse helpers
// ============================================================================

struct RawNode {
    node_id:     String,
    browse_name: String,
    browse_path: Option<String>,
    is_variable: bool,
}

fn hierarchical_desc(node_id: NodeId) -> BrowseDescription {
    BrowseDescription {
        node_id,
        browse_direction: BrowseDirection::Forward,
        reference_type_id: ReferenceTypeId::HierarchicalReferences.into(),
        include_subtypes: true,
        node_class_mask: NodeClassMask::all().bits(),
        result_mask: BrowseResultMask::All as u32,
    }
}

/// Iterative BFS browse starting from `start_node`, up to 6 levels deep.
/// Returns every node reachable via HierarchicalReferences.
async fn browse_tree(
    session:    &Arc<opcua::client::Session>,
    start_node: NodeId,
) -> Result<Vec<RawNode>, opcua::types::StatusCode> {
    // (node_id, parent_path, remaining_depth)
    let mut queue: VecDeque<(NodeId, Option<String>, u32)> = VecDeque::new();
    queue.push_back((start_node, None, 6));
    let mut all_nodes = Vec::new();

    while let Some((node_id, parent_path, depth)) = queue.pop_front() {
        if depth == 0 { continue; }

        let results = session.browse(&[hierarchical_desc(node_id)], 1000, None).await?;

        let Some(result) = results.into_iter().next() else { continue };
        let refs = result.references.unwrap_or_default();

        for r in refs {
            let name = r.browse_name.name.value().as_deref().unwrap_or("").to_string();
            let current_path = match &parent_path {
                Some(p) => Some(format!("{}/{}", p, name)),
                None    => Some(name.clone()),
            };
            let is_variable = r.node_class == NodeClass::Variable;
            let child_id    = r.node_id.node_id.clone();

            all_nodes.push(RawNode {
                node_id:     child_id.to_string(),
                browse_name: name,
                browse_path: current_path.clone(),
                is_variable,
            });

            if !is_variable {
                queue.push_back((child_id, current_path, depth - 1));
            }
        }
    }

    Ok(all_nodes)
}

/// Extract device_id and metric_name from a browse path like "HeatingPLC/boiler_001/temperature".
/// Skips the leading PLC folder name.
fn path_to_device_metric(path: &Option<String>) -> Option<(String, String)> {
    let p = path.as_deref()?;
    let parts: Vec<&str> = p.split('/').collect();
    // Expected: [plc_folder, device_id, metric_name]
    if parts.len() >= 3 {
        Some((parts[parts.len() - 2].to_string(), parts[parts.len() - 1].to_string()))
    } else {
        None
    }
}

// ============================================================================
// Value decoding helpers
// ============================================================================

fn data_type_from_variant(v: &Variant) -> NodeDataType {
    if let Variant::NodeId(id) = v {
        if id.namespace == 0 {
            if let Identifier::Numeric(n) = &id.identifier {
                return match n {
                    1                => NodeDataType::Boolean,
                    12               => NodeDataType::Str,
                    _                => NodeDataType::Float,
                };
            }
        }
    }
    NodeDataType::Float
}

fn datatype_to_variant(value: &DataType) -> Variant {
    match value {
        DataType::Float(f)   => Variant::Double(*f),
        DataType::Str(s)     => Variant::String(s.clone().into()),
        DataType::Boolean(b) => Variant::Boolean(*b),
    }
}

fn extract_value(dv: &DataValue, data_type: &NodeDataType) -> Result<DataType, String> {
    let variant = dv.value.as_ref().ok_or("no value in DataValue")?;
    match data_type {
        NodeDataType::Float => {
            let f = match variant {
                Variant::Double(v) => *v,
                Variant::Float(v)  => *v as f64,
                Variant::Int32(v)  => *v as f64,
                Variant::UInt32(v) => *v as f64,
                Variant::Int64(v)  => *v as f64,
                other => return Err(format!("expected numeric variant, got {:?}", other)),
            };
            Ok(DataType::Float(f))
        }
        NodeDataType::Str => match variant {
            Variant::String(s) => Ok(DataType::Str(s.value().as_deref().unwrap_or("").to_string())),
            other => Err(format!("expected string variant, got {:?}", other)),
        },
        NodeDataType::Boolean => match variant {
            Variant::Boolean(b) => Ok(DataType::Boolean(*b)),
            other => Err(format!("expected boolean variant, got {:?}", other)),
        },
    }
}
