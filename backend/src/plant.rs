use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config_handle::PlantConfigHandle;
use crate::comms::{GenericConnector, IngestedState, ScadaPlcConnector, release_ports};
use crate::simulator::SimulatorModule;

/// Boot the full plant: start the physics tick loop (once, if any PLCs are simulated),
/// then for each PLC start its OPC-UA simulator server (if simulated) and its connector.
/// Returns the shared IngestedState that all connectors write into.
pub async fn start(
    handle:  Arc<RwLock<PlantConfigHandle>>,
    tick_ms: u64,
) -> Result<Arc<RwLock<IngestedState>>, Box<dyn std::error::Error>> {
    let (plcs, endpoints, plant_name) = {
        let h = handle.read().await;
        (h.all_plcs().to_vec(), h.endpoint_configs(), h.plant_config().name.clone())
    };

    let sim_count  = plcs.iter().filter(|p| p.simulated).count();
    let live_count = plcs.len() - sim_count;
    let dev_count: usize = plcs.iter().map(|p| p.devices.len()).sum();

    let mut msg = format!(
        "\n  Plant: \"{}\"  ·  {} PLC{}  ({} simulated, {} live)  ·  {} device{}\n",
        plant_name,
        plcs.len(), if plcs.len() == 1 { "" } else { "s" },
        sim_count, live_count,
        dev_count, if dev_count == 1 { "" } else { "s" },
    );
    for plc in &plcs {
        let url = format!("{}:{}{}", plc.uri, plc.port, plc.endpoint);
        let tag = if plc.simulated { "[sim] " } else { "[live]" };
        let dev_ids: Vec<&str> = plc.devices.iter().map(|d| d.device_id.as_str()).collect();
        msg.push_str(&format!("\n  {}  {:20}  {}\n", tag, plc.name, url));
        msg.push_str(&format!("               {}\n", dev_ids.join(", ")));
    }
    msg.push('\n');
    tracing::info!("{}", msg);

    if plcs.iter().any(|p| p.simulated) {
        SimulatorModule::start_physics(Arc::clone(&handle), tick_ms).await?;
    }

    let ingested: Arc<RwLock<IngestedState>> = Arc::new(RwLock::new(HashMap::new()));

    for (plc, endpoint) in plcs.into_iter().zip(endpoints) {
        if plc.simulated {
            release_ports(&[plc.port]);
            SimulatorModule::start_server(Arc::clone(&handle), plc, tick_ms).await?;
        }

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
