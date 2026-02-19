use serde::{Deserialize, Serialize};
use crate::models::DeviceFieldValue;
use crate::models::devices::{DeviceSchema, DataType};
use super::physics_functions::{PhysicsFunction, PhysicsFunctionConfig, create_physics_function};
use super::device_functions::{DeviceFunction, DeviceFunctionConfig, create_device_function};
use std::collections::HashMap;
use std::sync::Arc;

/// Trait for devices to expose their fields dynamically
pub trait DeviceFields {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue>;
}

/// Device categories inspired by stream processing (Flink-style)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCategory {
    Source,     // 0 inputs → N outputs (e.g., boiler, sensors)
    Transform,  // N inputs → M outputs (e.g., valve, mixer)
    Sink,       // N inputs → 0 outputs (e.g., display, logger)
}

/// Input port configuration - defines where data comes from
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputPort {
    pub name: String,
    pub source_device_id: String,
    pub source_field: String,
}

/// Output port configuration - defines what data this device produces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPort {
    pub name: String,
    pub target_field: String,
}

/// Generic device with flexible I/O and pluggable physics
#[derive(Clone)]
pub struct Device {
    pub id: String,
    pub device_type: String,
    pub category: DeviceCategory,
    fields: HashMap<String, DeviceFieldValue>,
    input_ports: Vec<InputPort>,
    output_ports: Vec<OutputPort>,
    physics_function: Arc<dyn PhysicsFunction>,
    functions: HashMap<String, Arc<dyn DeviceFunction>>,
    #[allow(dead_code)]
    schema: Option<DeviceSchema>,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("id", &self.id)
            .field("device_type", &self.device_type)
            .field("category", &self.category)
            .field("fields", &self.fields)
            .field("input_ports", &self.input_ports)
            .field("output_ports", &self.output_ports)
            .field("physics_function", &"<dyn PhysicsFunction>")
            .field("functions", &format!("{} functions", self.functions.len()))
            .finish()
    }
}

impl Device {
    pub fn new(
        id: String,
        device_type: String,
        category: DeviceCategory,
        schema: DeviceSchema,
        input_ports: Vec<InputPort>,
        output_ports: Vec<OutputPort>,
        physics_config: PhysicsFunctionConfig,
        function_configs: Vec<(String, DeviceFunctionConfig)>,
        initial_values: HashMap<String, DeviceFieldValue>,
    ) -> Result<Self, String> {
        let mut fields = HashMap::new();
        for field_schema in &schema.fields {
            let value = initial_values
                .get(&field_schema.name)
                .cloned()
                .unwrap_or_else(|| match field_schema.data_type {
                    DataType::Double => DeviceFieldValue::Float(0.0),
                    DataType::String => DeviceFieldValue::String("Unknown".to_string()),
                });
            fields.insert(field_schema.name.clone(), value);
        }

        let physics_function = create_physics_function(physics_config);

        let functions = function_configs
            .into_iter()
            .map(|(name, config)| (name, create_device_function(config)))
            .collect();

        Ok(Self {
            id,
            device_type,
            category,
            fields,
            input_ports,
            output_ports,
            physics_function,
            functions,
            schema: Some(schema),
        })
    }

    /// Tick the device - compute outputs using physics function
    pub fn tick(&mut self, inputs: &HashMap<String, DeviceFieldValue>, dt: f64) {
        let outputs = self.physics_function.compute(self, inputs, dt);
        for (field_name, value) in outputs {
            self.fields.insert(field_name, value);
        }
    }

    /// Call a named control function (e.g., "open", "set_position")
    pub fn call_function(&mut self, name: &str, args: Vec<DeviceFieldValue>) -> Result<(), String> {
        let func = self.functions.get(name)
            .cloned()
            .ok_or_else(|| format!("Function '{}' not found on device '{}'", name, self.id))?;
        func.execute(self, args)
    }

    pub fn get_field(&self, name: &str) -> Option<DeviceFieldValue> {
        self.fields.get(name).cloned()
    }

    pub fn set_field(&mut self, name: String, value: DeviceFieldValue) {
        self.fields.insert(name, value);
    }

    pub fn get_float(&self, name: &str) -> Option<f64> {
        match self.fields.get(name)? {
            DeviceFieldValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn get_all_fields(&self) -> HashMap<String, DeviceFieldValue> {
        self.fields.clone()
    }

    pub fn get_input_ports(&self) -> &[InputPort] {
        &self.input_ports
    }

    pub fn get_output_ports(&self) -> &[OutputPort] {
        &self.output_ports
    }
}

impl DeviceFields for Device {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        self.fields.get(field_name).cloned()
    }
}
