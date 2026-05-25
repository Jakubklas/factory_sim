use std::collections::HashMap;
use crate::primitives::DataType;
use crate::schema::{
    DeviceConfig, DeviceTypeDefinition, PlantConfig,
    PlcEndpointConfig, NodeReadConfig,
};

/// A fully-resolved device: instance config merged with its type definition.
/// Pre-computed at startup so the tick loop and plc_server need no further lookups.
pub struct ResolvedDevice {
    pub config:   DeviceConfig,
    pub type_def: DeviceTypeDefinition,
}

/// Immutable resolved plant: static config + pre-resolved devices.
/// Replace Arc<RwLock<PlantConfigHandle>> with Arc<ResolvedPlant> on the backend.
pub struct ResolvedPlant {
    pub config:  PlantConfig,
    pub devices: Vec<ResolvedDevice>,
}

impl ResolvedPlant {
    /// Build from already-parsed config + type definitions.
    /// Validates that all instance params satisfy their type's required_params.
    pub fn build(
        config:       PlantConfig,
        device_types: Vec<DeviceTypeDefinition>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let registry: HashMap<String, DeviceTypeDefinition> = device_types
            .into_iter()
            .map(|t| (t.device_type.clone(), t))
            .collect();

        // Build device_id → plc_id map up front so input_variables can be validated
        // to stay within a single PLC (real PLCs are separate boxes — cross-PLC
        // wiring would have to traverse OPC-UA, which the physics engine cannot do).
        let device_to_plc: HashMap<String, String> = config.plcs.iter()
            .flat_map(|plc| plc.devices.iter().map(move |d| (d.device_id.clone(), plc.plc_id.clone())))
            .collect();

        let mut devices: Vec<ResolvedDevice> = Vec::new();

        for plc in &config.plcs {
            for device_config in &plc.devices {
                let type_def = registry
                    .get(&device_config.device_type)
                    .ok_or_else(|| format!(
                        "Device '{}' references unknown type '{}'",
                        device_config.device_id, device_config.device_type
                    ))?;

                for param in &type_def.required_params {
                    if !device_config.params.contains_key(&param.name) && param.default.is_none() {
                        return Err(format!(
                            "Device '{}' (type '{}') is missing required param '{}'",
                            device_config.device_id, device_config.device_type, param.name
                        ).into());
                    }
                }

                for input in &device_config.input_variables {
                    let source_plc = device_to_plc.get(&input.source_device_id)
                        .ok_or_else(|| format!(
                            "Device '{}' input '{}' references unknown source device '{}'",
                            device_config.device_id, input.name, input.source_device_id
                        ))?;
                    if source_plc != &plc.plc_id {
                        return Err(format!(
                            "Device '{}' (on PLC '{}') input '{}' references device '{}' on PLC '{}' — \
                             cross-PLC physics wiring is not allowed; move one of the devices or \
                             expose the value via a separate OPC-UA hop",
                            device_config.device_id, plc.plc_id, input.name,
                            input.source_device_id, source_plc
                        ).into());
                    }
                }

                devices.push(ResolvedDevice {
                    config:   device_config.clone(),
                    type_def: type_def.clone(),
                });
            }
        }

        Ok(Self { config, devices })
    }

    /// Return a new ResolvedPlant containing only the named PLC and its devices.
    /// Used by the simulator to scope a process to a single PLC.
    pub fn slice_to_plc(self, plc_id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let plc = self.config.plcs.iter()
            .find(|p| p.plc_id == plc_id)
            .ok_or_else(|| format!("PLC '{}' not found in plant.json", plc_id))?
            .clone();

        let device_ids: Vec<String> = plc.devices.iter().map(|d| d.device_id.clone()).collect();
        let devices = self.devices.into_iter()
            .filter(|d| device_ids.contains(&d.config.device_id))
            .collect();

        let mut config = self.config;
        config.plcs = vec![plc];

        Ok(Self { config, devices })
    }

    /// Build one PlcEndpointConfig per PLC — the connector layer uses this to know what to poll.
    /// Node ID format must match what plc_server registers in its address space.
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
