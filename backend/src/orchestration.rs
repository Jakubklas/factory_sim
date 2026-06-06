use std::sync::Arc;
use tokio::sync::RwLock;

use crate::comms::{IngestedState, WriteCmd};
use crate::db::plant_loader::{load_wires, OrchestratorWire};
use crate::plant::WriteHandles;

/// Start the wiring orchestration tick.
///
/// Wire table is loaded from DB at start. The caller may replace the inner Vec
/// (via `Arc<RwLock<Vec<OrchestratorWire>>>`) when a Postgres NOTIFY fires — the
/// next tick picks up the new table automatically.
///
/// Every `tick_ms` milliseconds, for each wire:
///   1. Read upstream value from `IngestedState` (previous-tick snapshot).
///   2. Write it into the downstream device's input port via that PLC's WriteHandle.
///
/// All wires are routed through the backend uniformly (same-PLC and cross-PLC).
pub fn start(
    wires:         Arc<RwLock<Vec<OrchestratorWire>>>,
    tick_ms:       u64,
    ingested:      Arc<RwLock<IngestedState>>,
    write_handles: WriteHandles,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
        loop {
            interval.tick().await;

            let wires_snapshot    = wires.read().await.clone();
            let ingested_snapshot = ingested.read().await.clone();
            let handles_snapshot  = write_handles.read().await;

            if wires_snapshot.is_empty() { continue; }

            for wire in &wires_snapshot {
                let Some(fields)     = ingested_snapshot.get(&wire.src_device_id) else { continue };
                let Some(data_type)  = fields.get(&wire.src_field)                else { continue };
                let Some(handle)     = handles_snapshot.get(&wire.dst_plc_name)   else {
                    tracing::trace!(
                        "[orchestration] No handle for PLC '{}' — skipping wire",
                        wire.dst_plc_name
                    );
                    continue;
                };
                handle.send(WriteCmd { node_id: wire.dst_node_id.clone(), value: data_type.clone() });
            }
        }
    });
}

/// Load wires from the DB and return them wrapped in Arc<RwLock<>>.
/// The LISTEN loop can call `refresh_wires` to swap in a new table.
pub async fn load(
    pool: &sqlx::PgPool,
) -> Result<Arc<RwLock<Vec<OrchestratorWire>>>, Box<dyn std::error::Error + Send + Sync>> {
    let wires = load_wires(pool).await?;
    tracing::info!("[orchestration] {} wire(s) loaded from DB", wires.len());
    if wires.is_empty() {
        tracing::info!("[orchestration] No wires configured — orchestration tick idle");
    }
    Ok(Arc::new(RwLock::new(wires)))
}

/// Reload wires from the DB and atomically update the shared table.
pub async fn refresh_wires(
    pool:  &sqlx::PgPool,
    wires: &Arc<RwLock<Vec<OrchestratorWire>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fresh = load_wires(pool).await?;
    tracing::info!("[orchestration] Wire table refreshed: {} wire(s)", fresh.len());
    *wires.write().await = fresh;
    Ok(())
}
