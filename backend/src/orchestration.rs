use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use plant_config::ResolvedPlant;

use crate::comms::{IngestedState, WriteCmd, WriteHandle};

/// Start the wiring orchestration tick.
///
/// Every `tick_ms` milliseconds, for each declared `input_variable` in the plant:
///   1. Look up the upstream device's output value from `IngestedState`.
///   2. Route it to the downstream device's input node via the appropriate PLC's `WriteHandle`.
///
/// This makes the backend the wiring router for *all* connections (same-PLC and cross-PLC
/// are treated identically — it is always an OPC-UA write into the downstream input port node).
///
/// The upstream read is from the previous-tick `IngestedState` snapshot; the downstream write
/// arrives in time for the *next* Jacobi tick — consistent with one-hop-per-tick semantics.
pub fn start(
    plant:         Arc<ResolvedPlant>,
    tick_ms:       u64,
    ingested:      Arc<RwLock<IngestedState>>,
    write_handles: Arc<HashMap<String, WriteHandle>>,
) {
    // Pre-compute the wire table once: (upstream_plc_name, upstream_device, upstream_field,
    //                                   downstream_plc_name, downstream_node_id)
    let wires = build_wire_table(&plant);
    if wires.is_empty() {
        tracing::info!("[orchestration] No wires configured — orchestration tick idle");
        return;
    }
    tracing::info!("[orchestration] {} wire(s) configured", wires.len());

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
        loop {
            interval.tick().await;

            let snapshot = ingested.read().await.clone();

            for wire in &wires {
                // Read upstream value
                let Some(device_fields) = snapshot.get(&wire.src_device_id) else { continue };
                let Some(data_type)     = device_fields.get(&wire.src_field)  else { continue };

                // Route to downstream
                let Some(handle) = write_handles.get(&wire.dst_plc_name) else {
                    tracing::trace!(
                        "[orchestration] No handle for PLC '{}' — skipping wire",
                        wire.dst_plc_name
                    );
                    continue;
                };

                handle.send(WriteCmd {
                    node_id: wire.dst_node_id.clone(),
                    value:   data_type.clone(),
                });
            }
        }
    });
}

// ============================================================================
// Internal wire record — pre-computed at start to avoid per-tick allocations
// ============================================================================

struct WireRecord {
    src_device_id: String,
    src_field:     String,
    dst_plc_name:  String,
    dst_node_id:   String,
}

/// Build the flat wire table from `plant.devices[*].config.input_variables`.
///
/// For each input variable we know:
///   - Source device + source field → look these up in `IngestedState` each tick.
///   - Destination device + input name + the PLC that owns the destination device
///     → compose the OPC-UA node path and pick the right WriteHandle.
///
/// Node ID format: "ns=2;s={plc_name}.{device_id}.{input_name}" — this matches
/// the node path registered by the simulator's `collect_input_specs()`.
fn build_wire_table(plant: &ResolvedPlant) -> Vec<WireRecord> {
    // device_id → plc_name (for both source lookup and destination routing)
    let device_to_plc: HashMap<&str, &str> = plant.config.plcs.iter()
        .flat_map(|plc| plc.devices.iter().map(move |d| (d.device_id.as_str(), plc.name.as_str())))
        .collect();

    let mut wires = Vec::new();

    for device in &plant.devices {
        let dst_device_id = &device.config.device_id;
        let Some(&dst_plc_name) = device_to_plc.get(dst_device_id.as_str()) else { continue };

        for input in &device.config.input_variables {
            // The node ID format must match collect_input_specs() in plc_server.rs:
            // "{plc_name}.{device_id}.{input_name}"  →  "ns=2;s={plc_name}.{device_id}.{input_name}"
            let dst_node_id = format!("ns=2;s={}.{}.{}", dst_plc_name, dst_device_id, input.name);

            wires.push(WireRecord {
                src_device_id: input.source_device_id.clone(),
                src_field:     input.source_field.clone(),
                dst_plc_name:  dst_plc_name.to_string(),
                dst_node_id,
            });
        }
    }

    wires
}
