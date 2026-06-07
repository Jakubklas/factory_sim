use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use plant_config::{PlcConfig, PlantConfig};

use crate::comms::{DiscoveredState, GenericConnector, IngestedState, ScadaPlcConnector, WriteHandle};

pub type WriteHandles = Arc<RwLock<HashMap<String, WriteHandle>>>;

/// Start one connector per PLC from the plant config.
/// Returns the shared ingested-state and a write-handle map (RwLock for dynamic insertion).
pub async fn start(
    plant:              Arc<PlantConfig>,
    tick_ms:            u64,
    discovered:         Arc<RwLock<DiscoveredState>>,
    pki_dir:            String,
    opcua_uri_override: Option<String>,
) -> Result<(Arc<RwLock<IngestedState>>, WriteHandles), Box<dyn std::error::Error>> {
    let plcs = &plant.plcs;

    let sim_count  = plcs.iter().filter(|p| p.simulated).count();
    let live_count = plcs.len() - sim_count;
    let dev_count: usize = plcs.iter().map(|p| p.devices.len()).sum();

    let mut msg = format!(
        "\n  Plant: \"{}\"  ·  {} PLC{}  ({} simulated, {} live)  ·  {} device{}\n",
        plant.name,
        plcs.len(), if plcs.len() == 1 { "" } else { "s" },
        sim_count, live_count,
        dev_count, if dev_count == 1 { "" } else { "s" },
    );
    for plc in plcs {
        let tag     = if plc.simulated { "[sim] " } else { "[live]" };
        let url     = format!("{}:{}{}", plc.endpoint_uri(), plc.port, plc.endpoint);
        let dev_ids: Vec<&str> = plc.devices.iter().map(|d| d.device_id.as_str()).collect();
        msg.push_str(&format!("\n  {}  {:20}  {}\n", tag, plc.name, url));
        msg.push_str(&format!("               {}\n", dev_ids.join(", ")));
    }
    msg.push('\n');
    tracing::info!("{}", msg);

    let ingested: Arc<RwLock<IngestedState>> = Arc::new(RwLock::new(HashMap::new()));
    let handles:  WriteHandles               = Arc::new(RwLock::new(HashMap::new()));

    let mut endpoints = plcs_to_endpoints(plcs);
    apply_uri_override(&mut endpoints, opcua_uri_override.as_deref());

    for endpoint in endpoints {
        if let Some((name, handle)) = make_connector(endpoint, tick_ms, Arc::clone(&ingested), Arc::clone(&discovered), pki_dir.clone()) {
            handles.write().await.insert(name, handle);
        }
    }

    Ok((ingested, handles))
}

/// Start a connector for a single newly-added PLC.
/// Called by the Postgres LISTEN loop when a new PLC row appears in the DB.
pub async fn add_plc_connector(
    plc:                &PlcConfig,
    tick_ms:            u64,
    ingested:           Arc<RwLock<IngestedState>>,
    discovered:         Arc<RwLock<DiscoveredState>>,
    handles:            WriteHandles,
    pki_dir:            String,
    opcua_uri_override: Option<String>,
) {
    if handles.read().await.contains_key(&plc.name) {
        return;
    }

    let url = format!("{}:{}{}", plc.endpoint_uri(), plc.port, plc.endpoint);
    let mut endpoints = vec![plant_config::PlcEndpointConfig {
        name:       plc.name.clone(),
        protocol:   plc.protocol.clone(),
        url,
        node_reads: vec![],
    }];
    apply_uri_override(&mut endpoints, opcua_uri_override.as_deref());

    for endpoint in endpoints {
        tracing::info!("[plant] Adding connector for new PLC: {}", endpoint.name);
        if let Some((name, handle)) = make_connector(endpoint, tick_ms, Arc::clone(&ingested), Arc::clone(&discovered), pki_dir.clone()) {
            handles.write().await.insert(name, handle);
        }
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

fn plcs_to_endpoints(plcs: &[PlcConfig]) -> Vec<plant_config::PlcEndpointConfig> {
    plcs.iter().map(|plc| {
        plant_config::PlcEndpointConfig {
            name:       plc.name.clone(),
            protocol:   plc.protocol.clone(),
            url:        format!("{}:{}{}", plc.endpoint_uri(), plc.port, plc.endpoint),
            node_reads: vec![],
        }
    }).collect()
}

fn make_connector(
    endpoint:   plant_config::PlcEndpointConfig,
    tick_ms:    u64,
    ingested:   Arc<RwLock<IngestedState>>,
    discovered: Arc<RwLock<DiscoveredState>>,
    pki_dir:    String,
) -> Option<(String, WriteHandle)> {
    match endpoint.protocol.as_str() {
        "opcua" => {
            let (name, connector) = ScadaPlcConnector::from_endpoint_config(endpoint, pki_dir);
            let (generic, handle) = GenericConnector::new(
                name.clone(), connector, tick_ms,
                Arc::clone(&ingested),
                Arc::clone(&discovered),
            );
            generic.spawn();
            Some((name, handle))
        }
        other => {
            tracing::warn!("Skipping '{}': unknown protocol '{}'", endpoint.name, other);
            None
        }
    }
}

fn apply_uri_override(endpoints: &mut [plant_config::PlcEndpointConfig], override_host: Option<&str>) {
    let Some(host) = override_host.filter(|s| !s.is_empty()) else {
        return;
    };
    for e in endpoints {
        e.url = rewrite_host(&e.url, host);
    }
}

fn rewrite_host(url: &str, host: &str) -> String {
    let Some(scheme_end) = url.find("://") else { return url.to_string() };
    let after_scheme = scheme_end + 3;
    let Some(port_offset) = url[after_scheme..].find(':') else { return url.to_string() };
    format!("{}{}", host, &url[after_scheme + port_offset..])
}
