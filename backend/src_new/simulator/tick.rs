use std::collections::{HashMap, HashSet, VecDeque};
use crate::simulator::{PlantConfigHandle, PhysicsEngine};
use crate::models::PhysicsMode;

/// Pre-computed device execution order, built once at startup from the wiring graph.
/// Devices are sorted topologically so upstream outputs are always ready before
/// downstream physics runs.
pub struct TickPlan {
    order: Vec<String>,   // device_ids in execution order
}

impl TickPlan {
    /// Build the execution order from the current wiring.
    /// Returns an error if the wiring graph contains a cycle.
    pub fn build(handle: &PlantConfigHandle) -> Result<Self, String> {
        let devices = handle.resolved_devices();

        // Map device_id → list of device_ids it depends on (its upstream sources)
        let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
        for d in devices {
            let upstream: Vec<&str> = d.config.input_variables
                .iter()
                .map(|p| p.source_device_id.as_str())
                .collect();
            deps.insert(d.config.device_id.as_str(), upstream);
        }

        // Kahn's algorithm
        // in_degree = number of upstream devices not yet processed
        let mut in_degree: HashMap<&str, usize> = devices
            .iter()
            .map(|d| (d.config.device_id.as_str(), 0))
            .collect();

        // downstream: device_id → list of devices that depend on it
        let mut downstream: HashMap<&str, Vec<&str>> = HashMap::new();
        for (id, upstream_ids) in &deps {
            for &up in upstream_ids {
                downstream.entry(up).or_default().push(id);
                *in_degree.entry(id).or_insert(0) += 1;
            }
        }

        // Start with devices that have no upstream dependencies
        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut order: Vec<String> = Vec::with_capacity(devices.len());
        let mut visited: HashSet<&str> = HashSet::new();

        while let Some(id) = queue.pop_front() {
            if visited.contains(id) { continue; }
            visited.insert(id);
            order.push(id.to_string());

            if let Some(dependents) = downstream.get(id) {
                for &dep in dependents {
                    let deg = in_degree.entry(dep).or_insert(0);
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push_back(dep);
                    }
                }
            }
        }

        if order.len() != devices.len() {
            return Err(format!(
                "Wiring cycle detected — only {} of {} devices could be ordered",
                order.len(), devices.len()
            ));
        }

        Ok(Self { order })
    }

    pub fn order(&self) -> &[String] {
        &self.order
    }
}

/// Run one simulation tick across all devices in topological order.
///
/// For each device:
///   1. Copy input variable values from upstream devices' live state into this device's state.
///      (e.g. Valve's outlet_pressure → FlowMeter's inlet_pressure)
///   2. Run the physics script via PhysicsEngine.
///   3. Write the updated state back into PlantConfigHandle.
///
/// Live devices (PhysicsMode::Live) skip step 2 — their state is written
/// directly by the OPC-UA reader in comms/.
pub fn tick(
    handle:  &mut PlantConfigHandle,
    plan:    &TickPlan,
    physics: &PhysicsEngine,
    dt:      f64,
) {
    for device_id in &plan.order {
        // --- 1. Propagate input port values from upstream state ---
        let input_copies: Vec<(String, crate::models::DataType)> = {
            let device = match handle.get_resolved(device_id) {
                Some(d) => d,
                None    => continue,
            };
            device.config.input_variables.iter().filter_map(|port| {
                handle
                    .get_field(&port.source_device_id, &port.source_field)
                    .map(|v| (port.name.clone(), v.clone()))
            }).collect()
        };

        for (field, value) in input_copies {
            handle.set_field(device_id, &field, value);
        }

        // --- 2. Run physics script ---
        let (device_type, physics_mode, params) = match handle.get_resolved(device_id) {
            Some(d) => (
                d.config.device_type.clone(),
                d.type_def.physics_mode,
                d.config.params.clone(),
            ),
            None => continue,
        };

        // Live devices are owned by the OPC-UA reader — skip physics entirely
        if matches!(physics_mode, PhysicsMode::Live) {
            continue;
        }

        if let Some(mut device_state) = handle.get_device_state(device_id).cloned() {
            if let Err(e) = physics.run(&device_type, &mut device_state, &params, dt) {
                tracing::warn!("Physics error on '{}': {}", device_id, e);
            }
            handle.set_device_state(device_id, device_state);
        }
    }
}
