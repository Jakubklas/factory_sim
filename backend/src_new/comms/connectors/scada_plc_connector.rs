use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use opcua::client::prelude::*;
use opcua::sync::RwLock as OpcRwLock;
use crate::models::DataType;
use crate::simulator::PlantHandle;
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
// Per-node read spec — built once at startup from PlantHandle schema
// ============================================================================

struct NodeRead {
    device_id:   String,
    metric_name: String,
    node_id:     String,     // "ns=2;s={plc_name}.{device_id}.{metric_name}"
    data_type:   NodeDataType,
}

enum NodeDataType { Float, Str, Boolean }

// ============================================================================
// ScadaPlcConnector — one instance per PLC endpoint
// ============================================================================

pub struct ScadaPlcConnector {
    plc_name:   String,
    endpoint:   String,
    node_reads: Vec<NodeRead>,
}

impl ScadaPlcConnector {
    pub fn new(plc: &PlcConfig, handle: &PlantHandle) -> Self {
        let endpoint = format!("{}:{}{}", plc.uri, plc.port, plc.endpoint);

        let node_reads = handle.resolved_devices()
            .iter()
            .filter(|d| plc.devices.iter().any(|pd| pd.device_id == d.config.device_id))
            .flat_map(|d| {
                d.type_def.metrics.iter().map(move |m| {
                    let data_type = match &m.data_type {
                        DataType::Float(_)   => NodeDataType::Float,
                        DataType::Str(_)     => NodeDataType::Str,
                        DataType::Boolean(_) => NodeDataType::Boolean,
                    };
                    NodeRead {
                        device_id:   d.config.device_id.clone(),
                        metric_name: m.name.clone(),
                        node_id:     format!("ns=2;s={}.{}.{}", plc.name, d.config.device_id, m.name),
                        data_type,
                    }
                })
            })
            .collect();

        Self { plc_name: plc.name.clone(), endpoint, node_reads }
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
            let value = read_node(&s, &node.node_id, &node.data_type)
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    format!("{}.{}: {}", self.plc_name, node.metric_name, e).into()
                })?;
            partial
                .entry(node.device_id.clone())
                .or_default()
                .insert(node.metric_name.clone(), value);
        }

        Ok(partial)
    }
}


// ============================================================================
// OPC-UA helpers
// ============================================================================

/// Single-attempt connect. Backoff and retry are handled by the generic runner.
fn connect_to_plc(
    endpoint: &str,
) -> Result<(Client, Arc<OpcRwLock<Session>>), Box<dyn std::error::Error + Send + Sync>> {
    let mut client = ClientBuilder::new()
        .application_name("factory-sim-scada")
        .application_uri("urn:factory-sim-scada")
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_limit(3)
        .client()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })?;

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
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })?;

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
