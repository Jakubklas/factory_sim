use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::simulator::PlantHandle;

pub mod generic_connector;
pub mod connectors;
pub mod servers;
pub mod plc_server;

pub use generic_connector::{IngestedState, GenericConnector};

/// Start one connector thread per PLC endpoint defined in the plant config.
/// Returns the shared IngestedState handle for ws_bridge to read from.
pub async fn start_connectors(
    handle: Arc<RwLock<PlantHandle>>,
) -> Result<Arc<RwLock<IngestedState>>, Box<dyn std::error::Error>> {
    let ingested: Arc<RwLock<IngestedState>> = Arc::new(RwLock::new(HashMap::new()));

    let plc_connectors = {
        let h = handle.read().await;
        connectors::build_scada_connectors(&h)
    };
    let tick_ms = handle.read().await.default_tick_ms();

    tracing::info!(
        "Starting {} SCADA connector(s)",
        plc_connectors.len()
    );

    for connector in plc_connectors {
        GenericConnector::new(connector, tick_ms, Arc::clone(&ingested)).start();
    }

    Ok(ingested)
}
