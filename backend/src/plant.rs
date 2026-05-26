use std::sync::Arc;
use tokio::sync::RwLock;
use plant_config::ResolvedPlant;

use crate::comms::{GenericConnector, IngestedState, ScadaPlcConnector};

/// Start one connector per PLC and return the shared ingested-state map.
/// Simulated PLCs are served by the simulator process — polled over OPC-UA identically to real ones.
pub async fn start(
    plant:   Arc<ResolvedPlant>,
    tick_ms: u64,
) -> Result<Arc<RwLock<IngestedState>>, Box<dyn std::error::Error>> {
    let plcs          = &plant.config.plcs;
    let mut endpoints = plant.endpoint_configs();

    // OPCUA_URI_OVERRIDE rewrites the host portion of every OPC-UA URL.
    // Use case: plant.json defaults the host to opc.tcp://{plc_id} (works under Docker
    // service-name DNS), but on a developer's machine we want to hit opc.tcp://localhost
    // instead. The override is applied here so connectors and log lines stay consistent.
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

    let ingested: Arc<RwLock<IngestedState>> = Arc::new(RwLock::new(std::collections::HashMap::new()));

    for endpoint in endpoints {
        match endpoint.protocol.as_str() {
            "opcua" => {
                let (name, connector) = ScadaPlcConnector::new(endpoint);
                GenericConnector::new(name, connector, tick_ms, Arc::clone(&ingested)).start();
            }
            other => tracing::warn!("Skipping '{}': unknown protocol '{}'", endpoint.name, other),
        }
    }

    Ok(ingested)
}

/// Replace the host portion of an OPC-UA URL while keeping the port and path.
/// Input:  "opc.tcp://old-host:4840/path"  +  "opc.tcp://new-host"
/// Output: "opc.tcp://new-host:4840/path"
/// Falls back to the original URL if it doesn't match the expected scheme://host:port pattern.
fn rewrite_host(url: &str, host: &str) -> String {
    let Some(scheme_end) = url.find("://") else { return url.to_string() };
    let after_scheme = scheme_end + 3;
    let Some(port_offset) = url[after_scheme..].find(':') else { return url.to_string() };
    format!("{}{}", host, &url[after_scheme + port_offset..])
}
