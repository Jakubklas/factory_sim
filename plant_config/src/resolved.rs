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

                devices.push(ResolvedDevice {
                    config:   device_config.clone(),
                    type_def: type_def.clone(),
                });
            }
        }

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
