use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::models::{DataType, DeviceConfig, DeviceTypeDefinition, PlantConfig, PlcConfig, PlcEndpointConfig, NodeReadConfig};
use super::{DeviceTypeRegistry, PlantRegistry};

// Type aliases to keep signatures readable
type DeviceId  = String;
type FieldName = String;
type LiveState = HashMap<DeviceId, HashMap<FieldName, DataType>>;

/// A fully-resolved device: instance config merged with its type definition.
/// This is what the tick loop and plc_server work with — no further lookups needed.
pub struct ResolvedDevice {
    pub config: DeviceConfig,               // instance: id, wiring, params, tick_ms
    pub type_def: DeviceTypeDefinition,     // type: physics, functions, metrics
}

impl ResolvedDevice {
    pub fn effective_tick_ms(&self, default: u64) -> u64 {
        self.config.tick_ms.unwrap_or(default)
    }
}

/// Runtime handle for a loaded plant.
/// Owns the static config tree, the type registry, resolved devices, and all live state.
/// Wrap in Arc<RwLock<PlantConfigHandle>> and share across threads.
pub struct PlantConfigHandle {
    config:   PlantConfig,
    registry: HashMap<String, DeviceTypeDefinition>,  // device_type → definition
    devices:  Vec<ResolvedDevice>,                    // all devices, type already merged in
    state:    LiveState,                              // live field values, mutated every tick
}

impl PlantConfigHandle {
    // -------------------------------------------------------------------------
    // Loading
    // -------------------------------------------------------------------------

    /// Build the runtime handle from already-loaded config and registry.
    /// File I/O is handled upstream by DeviceTypeRegistry and PlantRegistry.
    /// Validates that all instance params satisfy their type's required_params.
    /// Seeds live state from each type's metric initial_values.
    pub fn new(
        type_registry: DeviceTypeRegistry,
        plant_store:   PlantRegistry,
    ) -> Result<Arc<RwLock<Self>>, Box<dyn std::error::Error>> {
        let config = plant_store.into_config();
        let registry: HashMap<String, DeviceTypeDefinition> = type_registry
            .into_types()
            .into_iter()
            .map(|t| (t.device_type.clone(), t))
            .collect();

        // --- resolve devices + validate params + seed live state ---
        let mut devices: Vec<ResolvedDevice> = Vec::new();
        let mut state:   LiveState           = HashMap::new();

        for plc in &config.plcs {
            for device_config in &plc.devices {
                // look up the type definition
                let type_def = registry
                    .get(&device_config.device_type)
                    .ok_or_else(|| format!(
                        "Device '{}' references unknown type '{}'",
                        device_config.device_id, device_config.device_type
                    ))?;

                // validate required params
                for param in &type_def.required_params {
                    if !device_config.params.contains_key(&param.name) {
                        match param.default {
                            Some(_) => {} // will use default, fine
                            None    => return Err(format!(
                                "Device '{}' (type '{}') is missing required param '{}'",
                                device_config.device_id, device_config.device_type, param.name
                            ).into()),
                        }
                    }
                }

                // seed live state from type's metrics
                let mut fields: HashMap<FieldName, DataType> = HashMap::new();
                for metric in &type_def.metrics {
                    let value = metric.initial_value.clone()
                        .unwrap_or_else(|| match &metric.data_type {
                            DataType::Float(_)   => DataType::Float(0.0),
                            DataType::Str(_)     => DataType::Str(String::new()),
                            DataType::Boolean(_) => DataType::Boolean(false),
                        });
                    fields.insert(metric.name.clone(), value);
                }
                state.insert(device_config.device_id.clone(), fields);

                devices.push(ResolvedDevice {
                    config:   device_config.clone(),
                    type_def: type_def.clone(),
                });
            }
        }

        Ok(Arc::new(RwLock::new(Self { config, registry, devices, state })))
    }

    // -------------------------------------------------------------------------
    // Resolved device access
    // -------------------------------------------------------------------------

    pub fn resolved_devices(&self) -> &[ResolvedDevice] {
        &self.devices
    }

    pub fn get_resolved(&self, device_id: &str) -> Option<&ResolvedDevice> {
        self.devices.iter().find(|d| d.config.device_id == device_id)
    }

    // -------------------------------------------------------------------------
    // Live state — reads
    // -------------------------------------------------------------------------

    pub fn get_field(&self, device_id: &str, field: &str) -> Option<&DataType> {
        self.state.get(device_id)?.get(field)
    }

    pub fn get_device_state(&self, device_id: &str) -> Option<&HashMap<FieldName, DataType>> {
        self.state.get(device_id)
    }

    /// Full snapshot of live state — used by plc_server and ws_bridge to broadcast.
    pub fn state_snapshot(&self) -> HashMap<DeviceId, HashMap<FieldName, DataType>> {
        self.state.clone()
    }

    // -------------------------------------------------------------------------
    // Live state — writes
    // -------------------------------------------------------------------------

    pub fn set_field(&mut self, device_id: &str, field: &str, value: DataType) {
        if let Some(fields) = self.state.get_mut(device_id) {
            fields.insert(field.to_string(), value);
        }
    }

    pub fn set_device_state(&mut self, device_id: &str, fields: HashMap<FieldName, DataType>) {
        self.state.insert(device_id.to_string(), fields);
    }

    // -------------------------------------------------------------------------
    // Config lookups — PLCs
    // -------------------------------------------------------------------------

    pub fn get_plc_by_id(&self, plc_id: &str) -> Option<&PlcConfig> {
        self.config.plcs.iter().find(|p| p.plc_id == plc_id)
    }

    pub fn get_plc_by_name(&self, name: &str) -> Option<&PlcConfig> {
        self.config.plcs.iter().find(|p| p.name == name)
    }

    // -------------------------------------------------------------------------
    // Config lookups — type registry
    // -------------------------------------------------------------------------

    pub fn get_type_def(&self, device_type: &str) -> Option<&DeviceTypeDefinition> {
        self.registry.get(device_type)
    }

    // -------------------------------------------------------------------------
    // Convenience
    // -------------------------------------------------------------------------

    pub fn plant_name(&self) -> &str {
        &self.config.name
    }

    pub fn default_tick_ms(&self) -> u64 {
        self.config.default_tick_ms
    }

    pub fn all_plcs(&self) -> &[PlcConfig] {
        &self.config.plcs
    }

    // -------------------------------------------------------------------------
    // Connector config — source of truth for the platform's connector layer
    // -------------------------------------------------------------------------

    /// Build one PlcEndpointConfig per PLC from the resolved plant config.
    /// This is what the connector layer uses to know what to poll and how.
    /// For simulated PLCs, the simulator starts servers at these same addresses.
    /// For real PLCs, connectors connect directly — no simulator involved.
    pub fn endpoint_configs(&self) -> Vec<PlcEndpointConfig> {
        self.config.plcs.iter().map(|plc| {
            let url = format!("{}:{}{}", plc.uri, plc.port, plc.endpoint);
            let plc_device_ids: Vec<&str> = plc.devices.iter()
                .map(|d| d.device_id.as_str())
                .collect();

            let node_reads = self.devices.iter()
                .filter(|d| plc_device_ids.contains(&d.config.device_id.as_str()))
                .flat_map(|d| {
                    d.type_def.metrics.iter().map(move |m| NodeReadConfig {
                        device_id:   d.config.device_id.clone(),
                        metric_name: m.name.clone(),
                        // Node ID format must match what plc_server registers in its address space
                        node_id:     format!("ns=2;s={}.{}.{}", plc.name, d.config.device_id, m.name),
                        data_type:   m.initial_value.clone().unwrap_or(DataType::Float(0.0)),
                    })
                })
                .collect();

            PlcEndpointConfig {
                name:     plc.name.clone(),
                protocol: plc.protocol.clone(),
                url,
                node_reads,
            }
        }).collect()
    }
}
