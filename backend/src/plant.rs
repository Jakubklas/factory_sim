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
    let plcs      = handle.read().await.all_plcs().to_vec();
    let endpoints = handle.read().await.endpoint_configs();

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
                tracing::info!("[opcua] {} → {}", endpoint.name, endpoint.url);
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
