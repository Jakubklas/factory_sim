use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use plant_config::ResolvedPlant;

use crate::comms::{DiscoveredState, GenericConnector, IngestedState, ScadaPlcConnector, WriteHandle};

/// Start one connector per PLC.
/// Returns the shared ingested-state, discovered-state, and a write handle per PLC name.
pub async fn start(
    plant:      Arc<ResolvedPlant>,
    tick_ms:    u64,
    discovered: Arc<RwLock<DiscoveredState>>,
) -> Result<(Arc<RwLock<IngestedState>>, HashMap<String, WriteHandle>), Box<dyn std::error::Error>> {
    let plcs          = &plant.config.plcs;
    let mut endpoints = plant.endpoint_configs();

    let host_override = std::env::var("OPCUA_URI_OVERRIDE").ok().filter(|s| !s.is_empty());
    if let Some(host) = &host_override {
        for e in &mut endpoints {
            e.url = rewrite_host(&e.url, host);
        }
    }

    let sim_count  = plcs.iter().filter(|p| p.simulated).count();
    let live_count = plcs.len() - sim_count;
    let dev_count: usize = plcs.iter().map(|p| p.devices.len()).sum();

    let mut msg = format!(
        "\n  Plant: \"{}\"  ·  {} PLC{}  ({} simulated, {} live)  ·  {} device{}\n",
        plant.config.name,
        plcs.len(), if plcs.len() == 1 { "" } else { "s" },
        sim_count, live_count,
        dev_count, if dev_count == 1 { "" } else { "s" },
    );
    for (plc, endpoint) in plcs.iter().zip(endpoints.iter()) {
        let tag = if plc.simulated { "[sim] " } else { "[live]" };
        let dev_ids: Vec<&str> = plc.devices.iter().map(|d| d.device_id.as_str()).collect();
        msg.push_str(&format!("\n  {}  {:20}  {}\n", tag, plc.name, endpoint.url));
        msg.push_str(&format!("               {}\n", dev_ids.join(", ")));
    }
    msg.push('\n');
    tracing::info!("{}", msg);

    let ingested: Arc<RwLock<IngestedState>> = Arc::new(RwLock::new(HashMap::new()));
    let mut write_handles: HashMap<String, WriteHandle> = HashMap::new();

    for endpoint in endpoints {
        match endpoint.protocol.as_str() {
            "opcua" => {
                let (name, connector) = ScadaPlcConnector::from_endpoint_config(endpoint);
                let (generic, handle) = GenericConnector::new(
                    name.clone(), connector, tick_ms,
                    Arc::clone(&ingested),
                    Arc::clone(&discovered),
                );
                write_handles.insert(name, handle);
                generic.spawn();
            }
            other => tracing::warn!("Skipping '{}': unknown protocol '{}'", endpoint.name, other),
        }
    }

    Ok((ingested, write_handles))
}

fn rewrite_host(url: &str, host: &str) -> String {
    let Some(scheme_end) = url.find("://") else { return url.to_string() };
    let after_scheme = scheme_end + 3;
    let Some(port_offset) = url[after_scheme..].find(':') else { return url.to_string() };
    format!("{}{}", host, &url[after_scheme + port_offset..])
}
