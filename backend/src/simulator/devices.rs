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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BoilerStatus {
    Off,
    Heating,
    Steady,
    Overheat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Boiler {
    pub id: String,
    pub temperature: f64,
    pub target_temperature: f64,
    pub pressure: f64,
    pub status: BoilerStatus,
}

impl Boiler {
    pub fn new(id: String, target_temperature: f64) -> Self {
        Self {
            id,
            temperature: 20.0,
            target_temperature,
            pressure: 0.0,
            status: BoilerStatus::Off,
        }
    }

    pub fn tick(&mut self, dt: f64) {
        use super::physics::{add_noise, temperature_to_pressure};

        // Ramp temperature toward target
        let temp_diff = self.target_temperature - self.temperature;
        let ramp_rate = 5.0; // degrees per second

        if temp_diff.abs() > 0.1 {
            let change = temp_diff.signum() * ramp_rate * dt;
            self.temperature += change;
            self.temperature = self.temperature.clamp(0.0, 150.0);
            self.status = BoilerStatus::Heating;
        } else {
            self.temperature = self.target_temperature;
            self.status = BoilerStatus::Steady;
        }

        // Check for overheat
        if self.temperature > 120.0 {
            self.status = BoilerStatus::Overheat;
        } else if self.temperature < 10.0 {
            self.status = BoilerStatus::Off;
        }

        // Calculate pressure from temperature
        self.pressure = add_noise(temperature_to_pressure(self.temperature), 2.0);
    }
}

impl DeviceFields for Boiler {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        match field_name {
            "temperature" => Some(DeviceFieldValue::Float(self.temperature)),
            "target_temperature" => Some(DeviceFieldValue::Float(self.target_temperature)),
            "pressure" => Some(DeviceFieldValue::Float(self.pressure)),
            "status" => Some(DeviceFieldValue::String(format!("{:?}", self.status))),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum MeterStatus {
    Normal,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressureMeter {
    pub id: String,
    pub pressure: f64,
    pub status: MeterStatus,
}

impl PressureMeter {
    pub fn new(id: String) -> Self {
        Self {
            id,
            pressure: 0.0,
            status: MeterStatus::Normal,
        }
    }

    pub fn tick(&mut self, upstream_pressure: f64) {
        use super::physics::add_noise;

        self.pressure = add_noise(upstream_pressure, 1.0);

        // Update status based on pressure
        if self.pressure > 4.5 {
            self.status = MeterStatus::Critical;
        } else if self.pressure > 3.5 {
            self.status = MeterStatus::Warning;
        } else {
            self.status = MeterStatus::Normal;
        }
    }
}

impl DeviceFields for PressureMeter {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        match field_name {
            "pressure" => Some(DeviceFieldValue::Float(self.pressure)),
            "status" => Some(DeviceFieldValue::String(format!("{:?}", self.status))),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FlowMeterStatus {
    Normal,
    Low,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowMeter {
    pub id: String,
    pub flow_rate: f64,
    pub total_volume: f64,
    pub status: FlowMeterStatus,
}

impl FlowMeter {
    pub fn new(id: String) -> Self {
        Self {
            id,
            flow_rate: 0.0,
            total_volume: 0.0,
            status: FlowMeterStatus::Normal,
        }
    }

    pub fn tick(&mut self, dt: f64, upstream_pressure: f64, valve_position: f64) {
        use super::physics::{add_noise, calculate_flow_rate};

        self.flow_rate = add_noise(calculate_flow_rate(upstream_pressure, valve_position), 2.0);
        self.total_volume += self.flow_rate * dt / 60.0; // Convert L/min to L

        // Update status based on flow rate
        if self.flow_rate > 40.0 {
            self.status = FlowMeterStatus::High;
        } else if self.flow_rate < 5.0 {
            self.status = FlowMeterStatus::Low;
        } else {
            self.status = FlowMeterStatus::Normal;
        }
    }
}

impl DeviceFields for FlowMeter {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        match field_name {
            "flow_rate" => Some(DeviceFieldValue::Float(self.flow_rate)),
            "total_volume" => Some(DeviceFieldValue::Float(self.total_volume)),
            "status" => Some(DeviceFieldValue::String(format!("{:?}", self.status))),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValveMode {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValveStatus {
    Open,
    Closed,
    Partial,
    Fault,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Valve {
    pub id: String,
    pub position: f64,
    pub mode: ValveMode,
    pub status: ValveStatus,
}

impl Valve {
    pub fn new(id: String) -> Self {
        Self {
            id,
            position: 0.5,
            mode: ValveMode::Auto,
            status: ValveStatus::Partial,
        }
    }

    pub fn tick(&mut self, upstream_pressure: f64) {
        // In auto mode, regulate based on pressure
        if matches!(self.mode, ValveMode::Auto) {
            let target_pressure = 3.0;
            if upstream_pressure > target_pressure + 0.5 {
                // Pressure too high, open valve more
                self.position = (self.position + 0.02).min(1.0);
            } else if upstream_pressure < target_pressure - 0.5 {
                // Pressure too low, close valve
                self.position = (self.position - 0.02).max(0.0);
            }
        }

        // Update status based on position
        self.status = if self.position > 0.8 {
            ValveStatus::Open
        } else if self.position < 0.2 {
            ValveStatus::Closed
        } else {
            ValveStatus::Partial
        };
    }
}

impl DeviceFields for Valve {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        match field_name {
            "position" => Some(DeviceFieldValue::Float(self.position)),
            "mode" => Some(DeviceFieldValue::String(format!("{:?}", self.mode))),
            "status" => Some(DeviceFieldValue::String(format!("{:?}", self.status))),
            _ => None,
        }
    }
}

// ============================================================================
// GENERIC DEVICE ARCHITECTURE (Stream-Based)
// ============================================================================

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
    pub name: String,              // Port name (e.g., "upstream_pressure")
    pub source_device_id: String,  // Which device provides this input
    pub source_field: String,      // Which field from that device
}

/// Output port configuration - defines what data this device produces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPort {
    pub name: String,         // Port name (e.g., "output_pressure")
    pub target_field: String, // Internal field to write to
}

/// Generic device with flexible I/O and pluggable physics
#[derive(Clone)]
pub struct Device {
    pub id: String,
    pub device_type: String,
    pub category: DeviceCategory,

    // Dynamic field storage
    fields: HashMap<String, DeviceFieldValue>,

    // I/O configuration
    input_ports: Vec<InputPort>,
    output_ports: Vec<OutputPort>,

    // Physics/logic (using Arc for clonability)
    physics_function: Arc<dyn PhysicsFunction>,

    // Control functions (using Arc for clonability)
    functions: HashMap<String, Arc<dyn DeviceFunction>>,

    // Schema reference (not serialized)
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
    /// Create a new generic device from configuration
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
        // Initialize fields from schema + initial values
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

        // Create physics function
        let physics_function = create_physics_function(physics_config);

        // Create control functions
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

    /// Tick the device - compute outputs from inputs using physics function
    pub fn tick(&mut self, inputs: &HashMap<String, DeviceFieldValue>, dt: f64) {
        // 1. Compute outputs using physics function
        let outputs = self.physics_function.compute(self, inputs, dt);

        // 2. Write outputs to internal fields
        for (field_name, value) in outputs {
            self.fields.insert(field_name, value);
        }
    }

    /// Call a named function on this device (e.g., "open", "set_position")
    pub fn call_function(&mut self, name: &str, args: Vec<DeviceFieldValue>) -> Result<(), String> {
        // Clone the Arc (cheap - just increments ref count) to avoid borrow conflicts
        let func = self.functions.get(name)
            .cloned()
            .ok_or_else(|| format!("Function '{}' not found on device '{}'", name, self.id))?;

        // Now we can execute with mutable access to self
        func.execute(self, args)
    }

    /// Get a field value by name
    pub fn get_field(&self, name: &str) -> Option<DeviceFieldValue> {
        self.fields.get(name).cloned()
    }

    /// Set a field value
    pub fn set_field(&mut self, name: String, value: DeviceFieldValue) {
        self.fields.insert(name, value);
    }

    /// Get a float field value (convenience method)
    pub fn get_float(&self, name: &str) -> Option<f64> {
        match self.fields.get(name)? {
            DeviceFieldValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Get all input ports
    pub fn get_input_ports(&self) -> &[InputPort] {
        &self.input_ports
    }

    /// Get all output ports
    pub fn get_output_ports(&self) -> &[OutputPort] {
        &self.output_ports
    }
}

impl DeviceFields for Device {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        self.fields.get(field_name).cloned()
    }
}
