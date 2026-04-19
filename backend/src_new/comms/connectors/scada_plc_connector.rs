use std::str::FromStr;
use std::sync::Arc;
use opcua::client::prelude::*;
use opcua::sync::RwLock as OpcRwLock;
use crate::models::{DataType, PlcEndpointConfig};
use crate::comms::generic_connector::{ConnectorImpl, PartialState};

// ============================================================================
// Connection handle — owns both Client and Session.
// Client must stay alive for the duration of the session.
// ============================================================================

pub struct PlcConnection {
    _client: Client,
    session: Arc<OpcRwLock<Session>>,
}

// ============================================================================
// Internal node read spec — built from PlcEndpointConfig at construction time
// ============================================================================

struct NodeRead {
    device_id:   String,
    metric_name: String,
    node_id:     String,
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
    /// Build from a PlcEndpointConfig — works for both simulated and real PLCs.
    /// No PlantConfigHandle or simulator knowledge needed.
    pub fn new(config: PlcEndpointConfig) -> (String, Self) {
        let node_reads = config.node_reads.into_iter().map(|n| NodeRead {
            device_id:   n.device_id,
            metric_name: n.metric_name,
            node_id:     n.node_id,
            data_type:   match n.data_type {
                DataType::Float(_)   => NodeDataType::Float,
                DataType::Str(_)     => NodeDataType::Str,
                DataType::Boolean(_) => NodeDataType::Boolean,
            },
        }).collect();

        let connector = Self {
            plc_name:   config.name.clone(),
            endpoint:   config.url,
            node_reads,
        };

        (config.name, connector)
    }
}

impl ConnectorImpl for ScadaPlcConnector {
    type Conn = PlcConnection;

    fn connect(&self) -> Result<PlcConnection, Box<dyn std::error::Error + Send + Sync>> {
        let (client, session) = connect_to_plc(&self.endpoint)?;
        Ok(PlcConnection { _client: client, session })
    }

    fn poll(&self, conn: &PlcConnection) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>> {
        let mut partial = PartialState::new();
        let s = conn.session.read();

        for node in &self.node_reads {
            match read_node(&s, &node.node_id, &node.data_type) {
                Ok(value) => {
                    tracing::debug!(
                        "{}.{}.{} = {:?}",
                        self.plc_name, node.device_id, node.metric_name, value
                    );
                    partial
                        .entry(node.device_id.clone())
                        .or_default()
                        .insert(node.metric_name.clone(), value);
                }
                Err(e) => {
                    // One failed read poisons the whole poll — triggers reconnect in GenericConnector.
                    // If this fires, check that node_id format matches what plc_server registers.
                    return Err(format!(
                        "[{}] node '{}' read failed: {}",
                        self.plc_name, node.node_id, e
                    ).into());
                }
            }
        }

        Ok(partial)
    }
}

// ============================================================================
// OPC-UA helpers
// ============================================================================

/// Single-attempt connect. Backoff and retry are handled by GenericConnector.
fn connect_to_plc(
    endpoint: &str,
) -> Result<(Client, Arc<OpcRwLock<Session>>), Box<dyn std::error::Error + Send + Sync>> {
    let pki_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("pki/clients/scada")))
        .unwrap_or_else(|| "pki/clients/scada".into());

    let mut client = ClientBuilder::new()
        .application_name("factory-sim-scada")
        .application_uri("urn:factory-sim-scada")
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_limit(3)
        .pki_dir(pki_dir)
        .client()
        .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
            "ClientBuilder::client() returned None — check OPC-UA config".into()
        })?;

    let session = client
        .connect_to_endpoint(
            (
                endpoint,
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
                UserTokenPolicy::anonymous(),
            ),
            IdentityToken::Anonymous,
        )
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { format!("{:?}", e).into() })?;

    Ok((client, session))
}

fn read_node(
    session:   &Session,
    node_id:   &str,
    data_type: &NodeDataType,
) -> Result<DataType, Box<dyn std::error::Error + Send + Sync>> {
    let node_id = NodeId::from_str(node_id)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("invalid node_id '{}': {:?}", node_id, e).into()
        })?;

    let results = session
        .read(&[ReadValueId::from(node_id)], TimestampsToReturn::Neither, 0.0)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("session.read failed: {:?}", e).into()
        })?;

    let variant = results
        .first()
        .and_then(|dv| dv.value.as_ref())
        .ok_or("no value returned")?;

    match data_type {
        NodeDataType::Float => {
            let f = match variant {
                Variant::Double(v) => *v,
                Variant::Float(v)  => *v as f64,
                Variant::Int32(v)  => *v as f64,
                _ => return Err("expected numeric variant".into()),
            };
            Ok(DataType::Float(f))
        }
        NodeDataType::Str => match variant {
            Variant::String(s) => Ok(DataType::Str(s.to_string())),
            _ => Err("expected string variant".into()),
        },
        NodeDataType::Boolean => match variant {
            Variant::Boolean(b) => Ok(DataType::Boolean(*b)),
            _ => Err("expected boolean variant".into()),
        },
    }
}
