use std::collections::HashMap;
use plant_config::{PhysicsMode, ResolvedDevice};
use crate::physics_definitions::PhysicsEngine;
use crate::state::SimulatorState;

/// Run one Jacobi tick:
///   1. Freeze the previous-tick snapshot — all devices read from it, not from live state.
///   2. Compute every device's new state from the frozen snapshot (order-independent).
///   3. Commit all computed states atomically (from the next tick's perspective).
///
/// Consequences:
///   - No topo-sort needed: processing order within a tick is irrelevant.
///   - Feedback loops are supported: they iterate tick-by-tick.
///   - Chain latency = depth × tick_ms; parallel branches update on the same tick.
///   - Input port values must be present in the snapshot (written by the backend via OPC-UA
///     or carried over from a previous write). Devices whose inputs are not all ready are
///     skipped (readiness gate).
pub fn tick(
    state:   &mut SimulatorState,
    devices: &[ResolvedDevice],
    physics: &PhysicsEngine,
    dt:      f64,
) {
    // 1. Frozen snapshot — previous-tick values only.
    let snapshot = state.snapshot();

    // 2. Compute each device independently.
    let mut new_states: HashMap<String, HashMap<String, plant_config::DataType>> = HashMap::new();

    for device in devices {
        let device_id = &device.config.device_id;

        if matches!(device.type_def.physics_mode, PhysicsMode::Live) {
            continue;
        }

        // Readiness gate: every declared input port must have a value in the snapshot.
        // Devices with no inputs are always ready (vacuously true).
        let ready = device.config.input_variables.iter().all(|input| {
            snapshot
                .get(device_id)
                .and_then(|fields| fields.get(&input.name))
                .is_some()
        });

        if !ready {
            tracing::trace!(
                "Device '{}' skipped — waiting for inputs: {:?}",
                device_id,
                device.config.input_variables.iter().map(|i| &i.name).collect::<Vec<_>>()
            );
            continue;
        }

        let Some(current_state) = snapshot.get(device_id) else { continue };
        let mut device_state = current_state.clone();

        if let Err(e) = physics.run(
            &device.config.device_type,
            &mut device_state,
            &device.config.params,
            dt,
        ) {
            tracing::warn!("Physics error on '{}': {}", device_id, e);
            continue;
        }

        new_states.insert(device_id.clone(), device_state);
    }

    // 3. Commit — all outputs become available for the next tick's snapshot.
    for (device_id, new_state) in new_states {
        state.set_device_state(&device_id, new_state);
    }
}
