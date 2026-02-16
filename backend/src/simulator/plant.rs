use super::devices::{Boiler, PressureMeter, FlowMeter, Valve, Device};
use crate::models::devices::{DeviceConfig, DeviceSchemaRegistry, Topology};
use crate::models::DeviceFieldValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlantState {
    #[serde(flatten)]
    pub devices: HashMap<String, DeviceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeviceState {
    Boiler(Boiler),
    PressureMeter(PressureMeter),
    FlowMeter(FlowMeter),
    Valve(Valve),
}

// OLD PLANT IMPLEMENTATION (will be removed in Phase 9)
pub struct OldPlant {
    pub boiler_1: Boiler,
    pub boiler_2: Boiler,
    pub pressure_meter_1: PressureMeter,
    pub flow_meter_1: FlowMeter,
    pub valve_1: Valve,
}

impl OldPlant {
    pub fn new() -> Self {
        Self {
            boiler_1: Boiler::new("boiler-1".to_string(), 85.0),
            boiler_2: Boiler::new("boiler-2".to_string(), 75.0),
            pressure_meter_1: PressureMeter::new("pressure-meter-1".to_string()),
            flow_meter_1: FlowMeter::new("flow-meter-1".to_string()),
            valve_1: Valve::new("valve-1".to_string()),
        }
    }

    pub fn tick(&mut self, dt: f64) {
        // Update devices in topology order following the plant flow
        // Boiler 1 → Pressure Meter 1 → Valve 1 → Flow Meter 1 → Boiler 2

        self.boiler_1.tick(dt);
        self.pressure_meter_1.tick(self.boiler_1.pressure);
        self.valve_1.tick(self.boiler_1.pressure);
        self.flow_meter_1.tick(dt, self.boiler_1.pressure, self.valve_1.position);
        self.boiler_2.tick(dt);
    }

    pub fn get_state(&self) -> PlantState {
        let mut devices = HashMap::new();
        devices.insert("boiler-1".to_string(), DeviceState::Boiler(self.boiler_1.clone()));
        devices.insert("boiler-2".to_string(), DeviceState::Boiler(self.boiler_2.clone()));
        devices.insert("pressure-meter-1".to_string(), DeviceState::PressureMeter(self.pressure_meter_1.clone()));
        devices.insert("flow-meter-1".to_string(), DeviceState::FlowMeter(self.flow_meter_1.clone()));
        devices.insert("valve-1".to_string(), DeviceState::Valve(self.valve_1.clone()));

        PlantState { devices }
    }
}

// ============================================================================
// NEW PLANT IMPLEMENTATION (Stream-Based Architecture)
// ============================================================================

/// New plant state for generic device architecture
#[derive(Debug, Clone)]
pub struct GenericPlantState {
    pub devices: HashMap<String, Device>,
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
        // Create devices from configs
        let mut devices = HashMap::new();

        for config in device_configs {
            let schema = schema_registry
                .get_schema(&config.device_type)
                .ok_or_else(|| format!("Unknown device type: {}", config.device_type))?
                .clone();

            // Convert initial values first (before moving any config fields)
            let initial_values = config.convert_initial_values();

            // Convert FunctionConfig to (String, DeviceFunctionConfig) tuples
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

        // Compute execution order (topological sort of input dependencies)
        let execution_order = topology.compute_execution_order(&devices)?;

        Ok(Self {
            devices,
            execution_order,
        })
    }

    /// Tick all devices in topological order using port-based I/O
    pub fn tick(&mut self, dt: f64) {
        for device_id in self.execution_order.clone() {
            // 1. Gather inputs from upstream devices via input ports
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

            // 2. Tick the device with gathered inputs
            let device = self.devices.get_mut(&device_id).unwrap();
            device.tick(&inputs, dt);
        }
    }

    /// Get a reference to a device by ID
    pub fn get_device(&self, id: &str) -> Option<&Device> {
        self.devices.get(id)
    }

    /// Get a mutable reference to a device by ID
    pub fn get_device_mut(&mut self, id: &str) -> Option<&mut Device> {
        self.devices.get_mut(id)
    }

    /// Get the current plant state (all devices)
    pub fn get_state(&self) -> GenericPlantState {
        GenericPlantState {
            devices: self.devices.clone(),
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
