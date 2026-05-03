use std::sync::Arc;
use tokio::sync::RwLock;
use plant_config::ResolvedPlant;

use crate::comms::{GenericConnector, IngestedState, ScadaPlcConnector};

/// Boot the plant connectors. For each PLC, start a connector that polls its OPC-UA endpoint.
/// Simulated PLCs are served by the separate simulator process — the backend polls them the same way.
/// Returns the shared IngestedState that all connectors write into.
pub async fn start(
    plant:   Arc<ResolvedPlant>,
    tick_ms: u64,
) -> Result<Arc<RwLock<IngestedState>>, Box<dyn std::error::Error>> {
    let plcs       = &plant.config.plcs;
    let endpoints  = plant.endpoint_configs();

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
    for plc in plcs {
        let url = format!("{}:{}{}", plc.uri, plc.port, plc.endpoint);
        let tag = if plc.simulated { "[sim] " } else { "[live]" };
        let dev_ids: Vec<&str> = plc.devices.iter().map(|d| d.device_id.as_str()).collect();
        msg.push_str(&format!("\n  {}  {:20}  {}\n", tag, plc.name, url));
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
            other => {
                tracing::warn!("Skipping '{}': unknown protocol '{}'", endpoint.name, other);
            }
        }
    }

    Ok(ingested)
}
