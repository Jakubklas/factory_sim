use std::collections::{HashMap, HashSet, VecDeque};
use plant_config::{PhysicsMode, ResolvedDevice};
use crate::physics_definitions::PhysicsEngine;
use crate::state::SimulatorState;

/// Pre-computed device execution order, built once at startup from the wiring graph.
pub struct TickPlan {
    order: Vec<String>,
}

impl TickPlan {
    pub fn build(devices: &[ResolvedDevice]) -> Result<Self, String> {
        let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
        for d in devices {
            let upstream: Vec<&str> = d.config.input_variables
                .iter()
                .map(|p| p.source_device_id.as_str())
                .collect();
            deps.insert(d.config.device_id.as_str(), upstream);
        }

        let mut in_degree: HashMap<&str, usize> = devices
            .iter()
            .map(|d| (d.config.device_id.as_str(), 0))
            .collect();

        let mut downstream: HashMap<&str, Vec<&str>> = HashMap::new();
        for (id, upstream_ids) in &deps {
            for &up in upstream_ids {
                downstream.entry(up).or_default().push(id);
                *in_degree.entry(id).or_insert(0) += 1;
            }
        }

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
pub fn tick(
    state:   &mut SimulatorState,
    devices: &[ResolvedDevice],
    plan:    &TickPlan,
    physics: &PhysicsEngine,
    dt:      f64,
) {
    for device_id in &plan.order {
        let device = match devices.iter().find(|d| &d.config.device_id == device_id) {
            Some(d) => d,
            None    => continue,
        };

        // 1. Propagate input port values from upstream state
        let input_copies: Vec<(String, plant_config::DataType)> = device
            .config.input_variables.iter().filter_map(|port| {
                state
                    .get_field(&port.source_device_id, &port.source_field)
                    .map(|v| (port.name.clone(), v.clone()))
            }).collect();

        for (field, value) in input_copies {
            state.set_field(device_id, &field, value);
        }

        // 2. Run physics script (skip Live devices)
        if matches!(device.type_def.physics_mode, PhysicsMode::Live) {
            continue;
        }

        let params = device.config.params.clone();
        let device_type = device.config.device_type.clone();

        if let Some(mut device_state) = state.get_device_state(device_id).cloned() {
            if let Err(e) = physics.run(&device_type, &mut device_state, &params, dt) {
                tracing::warn!("Physics error on '{}': {}", device_id, e);
            }
            state.set_device_state(device_id, device_state);
        }
    }
}
