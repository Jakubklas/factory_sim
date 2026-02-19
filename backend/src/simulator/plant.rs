use super::devices::Device;
use crate::models::devices::{DeviceConfig, DeviceSchemaRegistry, Topology};
use crate::models::DeviceFieldValue;
use serde::Serialize;
use std::collections::HashMap;

/// Serializable plant state - sent over WebSocket and broadcast channel
#[derive(Debug, Clone, Serialize)]
pub struct PlantState {
    pub devices: HashMap<String, HashMap<String, DeviceFieldValue>>,
}

/// New Plant with flexible device topology
pub struct Plant {
    devices: HashMap<String, Device>,
    execution_order: Vec<String>,
}

impl Plant {
    /// Create plant from device configurations and topology
    pub fn from_config(
        device_configs: Vec<DeviceConfig>,
        topology: Topology,
        schema_registry: &DeviceSchemaRegistry,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut devices = HashMap::new();

        for config in device_configs {
            let schema = schema_registry
                .get_schema(&config.device_type)
                .ok_or_else(|| format!("Unknown device type: {}", config.device_type))?
                .clone();

            let initial_values = config.convert_initial_values();

            let function_configs: Vec<(String, crate::simulator::device_functions::DeviceFunctionConfig)> = config
                .functions
                .into_iter()
                .map(|f| (f.name, f.config))
                .collect();

            let device = Device::new(
                config.id.clone(),
                config.device_type,
                config.category,
                schema,
                config.input_ports,
                config.output_ports,
                config.physics_function,
                function_configs,
                initial_values,
            )?;

            devices.insert(config.id, device);
        }

        let execution_order = topology.compute_execution_order(&devices)?;

        Ok(Self { devices, execution_order })
    }

    /// Tick all devices in topological order using port-based I/O
    pub fn tick(&mut self, dt: f64) {
        for device_id in self.execution_order.clone() {
            // Gather inputs from upstream devices via input ports
            let inputs = {
                let device = self.devices.get(&device_id).unwrap();
                let mut inputs = HashMap::new();

                for input_port in device.get_input_ports() {
                    if let Some(source_dev) = self.devices.get(&input_port.source_device_id) {
                        if let Some(value) = source_dev.get_field(&input_port.source_field) {
                            inputs.insert(input_port.name.clone(), value);
                        }
                    }
                }
                inputs
            };

            let device = self.devices.get_mut(&device_id).unwrap();
            device.tick(&inputs, dt);
        }
    }

    /// Get a reference to all devices
    pub fn get_devices(&self) -> &HashMap<String, Device> {
        &self.devices
    }

    /// Get a reference to a device by ID
    pub fn get_device(&self, id: &str) -> Option<&Device> {
        self.devices.get(id)
    }

    /// Get the current serializable plant state (field values only)
    pub fn get_state(&self) -> PlantState {
        PlantState {
            devices: self.devices
                .iter()
                .map(|(id, device)| (id.clone(), device.get_all_fields()))
                .collect(),
        }
    }

    /// Call a function on a specific device
    pub fn call_device_function(
        &mut self,
        device_id: &str,
        function_name: &str,
        args: Vec<DeviceFieldValue>,
    ) -> Result<(), String> {
        let device = self.devices.get_mut(device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        device.call_function(function_name, args)
    }
}
