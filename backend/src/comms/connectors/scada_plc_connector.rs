use std::str::FromStr;
use std::sync::Arc;
use async_trait::async_trait;
use opcua::client::{ClientBuilder, IdentityToken};
use opcua::types::{
    AttributeId, DataValue, EndpointDescription, MessageSecurityMode, NodeId,
    ReadValueId, TimestampsToReturn, UserTokenPolicy, Variant,
};
use plant_config::{DataType, PlcEndpointConfig};
use crate::comms::generic_connector::{ConnectorImpl, PartialState};

// ============================================================================
// Connection handle — owns the active session (Arc so poll can reference it).
// The event loop runs in a separate spawned task and handles reconnect.
// ============================================================================

pub struct PlcConnection {
    session: Arc<opcua::client::Session>,
}

// ============================================================================
// Internal node read spec — built from PlcEndpointConfig at construction time
// ============================================================================

struct NodeRead {
    device_id:   String,
    metric_name: String,
    node_id:     NodeId,
    data_type:   NodeDataType,
}

enum NodeDataType { Float, Str, Boolean }

// ============================================================================
// ScadaPlcConnector — one instance per OPC-UA endpoint
// ============================================================================

pub struct ScadaPlcConnector {
    endpoint:   String,
    plc_name:   String,
    node_reads: Vec<NodeRead>,
}

impl ScadaPlcConnector {
    pub fn new(config: PlcEndpointConfig) -> (String, Self) {
        let node_reads = config.node_reads.into_iter().filter_map(|n| {
            let node_id = NodeId::from_str(&n.node_id).ok()?;
            Some(NodeRead {
                device_id:   n.device_id,
                metric_name: n.metric_name,
                node_id,
                data_type:   match n.data_type {
                    DataType::Float(_)   => NodeDataType::Float,
                    DataType::Str(_)     => NodeDataType::Str,
                    DataType::Boolean(_) => NodeDataType::Boolean,
                },
            })
        }).collect();

        let connector = Self {
            plc_name: config.name.clone(),
            endpoint: config.url,
            node_reads,
        };
        (config.name, connector)
    }
}

#[async_trait]
impl ConnectorImpl for ScadaPlcConnector {
    type Conn = PlcConnection;

    async fn connect(&self) -> Result<PlcConnection, Box<dyn std::error::Error + Send + Sync>> {
        let base = std::env::var("PKI_DIR").unwrap_or_else(|_| "./pki".to_string());
        let pki_dir = std::path::PathBuf::from(base).join("clients").join("scada");

        let mut client = ClientBuilder::new()
            .application_name("factory-sim-scada")
            .application_uri("urn:factory-sim-scada")
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_limit(-1)   // infinite internal reconnect
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

        Ok(PlcConnection { session })
    }

    async fn poll(&self, conn: &PlcConnection) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>> {
        let node_ids: Vec<ReadValueId> = self.node_reads.iter()
            .map(|n| ReadValueId {
                node_id:      n.node_id.clone(),
                attribute_id: AttributeId::Value as u32,
                ..Default::default()
            })
            .collect();

        let results = conn.session
            .read(&node_ids, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(|e| format!("[{}] session.read failed: {:?}", self.plc_name, e))?;

        let mut partial = PartialState::new();

        for (node, dv) in self.node_reads.iter().zip(results.iter()) {
            let value = extract_value(dv, &node.data_type)
                .map_err(|e| format!("[{}] node '{}' read failed: {}", self.plc_name, node.node_id, e))?;

            tracing::trace!("{}.{}.{} = {}", self.plc_name, node.device_id, node.metric_name, value);
            partial
                .entry(node.device_id.clone())
                .or_default()
                .insert(node.metric_name.clone(), value);
        }

        Ok(partial)
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
