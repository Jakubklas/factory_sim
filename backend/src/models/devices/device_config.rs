use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::models::DeviceFieldValue;
use crate::simulator::devices::{DeviceCategory, InputPort, OutputPort};
use crate::simulator::physics_functions::PhysicsFunctionConfig;
use crate::simulator::device_functions::DeviceFunctionConfig;

/// Configuration for a single device instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub id: String,
    pub device_type: String,
    pub category: DeviceCategory,
    pub input_ports: Vec<InputPort>,
    pub output_ports: Vec<OutputPort>,
    pub physics_function: PhysicsFunctionConfig,
    pub functions: Vec<FunctionConfig>,
    pub initial_values: HashMap<String, serde_json::Value>,
}

impl DeviceConfig {
    /// Convert JSON values to DeviceFieldValue
    pub fn convert_initial_values(&self) -> HashMap<String, DeviceFieldValue> {
        self.initial_values
            .iter()
            .map(|(k, v)| {
                let field_value = match v {
                    serde_json::Value::Number(n) => {
                        if let Some(f) = n.as_f64() {
                            DeviceFieldValue::Float(f)
                        } else {
                            DeviceFieldValue::Float(0.0)
                        }
                    }
                    serde_json::Value::String(s) => DeviceFieldValue::String(s.clone()),
                    _ => DeviceFieldValue::String("Unknown".to_string()),
                };
                (k.clone(), field_value)
            })
            .collect()
    }
}

/// Wrapper for function configurations with names
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionConfig {
    pub name: String,
    #[serde(flatten)]
    pub config: DeviceFunctionConfig,
}

/// Registry of device configurations (loaded from devices.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfigRegistry {
    pub devices: Vec<DeviceConfig>,
}

impl DeviceConfigRegistry {
    /// Load device configurations from JSON file
    pub fn from_json(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        // Parse as raw JSON first, then deserialize each device individually for better errors
        let raw: serde_json::Value = serde_json::from_str(&content)?;
        let devices_raw = raw["devices"].as_array()
            .ok_or("devices.json missing 'devices' array")?;

        let mut devices = Vec::new();
        for raw_device in devices_raw {
            let id = raw_device["id"].as_str().unwrap_or("?");
            // Try each field individually to isolate failures
            for key in ["physics_function", "functions", "initial_values", "category", "input_ports", "output_ports"] {
                let v = raw_device[key].clone();
                let result = match key {
                    "physics_function" => serde_json::from_value::<crate::simulator::physics_functions::PhysicsFunctionConfig>(v).map(|_| ()),
                    "functions" => serde_json::from_value::<Vec<FunctionConfig>>(v).map(|_| ()),
                    "initial_values" => serde_json::from_value::<std::collections::HashMap<String, serde_json::Value>>(v).map(|_| ()),
                    _ => Ok(()),
                };
                if let Err(e) = result {
                    eprintln!("ERROR in device '{}' field '{}': {}", id, key, e);
                }
            }
            match serde_json::from_value::<DeviceConfig>(raw_device.clone()) {
                Ok(d) => devices.push(d),
                Err(e) => return Err(format!("Failed to parse device '{}': {}", id, e).into()),
            }
        }
        Ok(DeviceConfigRegistry { devices })
    }

    /// Get configuration for a specific device by ID
    pub fn get_device(&self, device_id: &str) -> Option<&DeviceConfig> {
        self.devices.iter().find(|d| d.id == device_id)
    }
}

/// Topology configuration - defines execution order of devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topology {
    pub execution_order: Vec<String>,
}

impl Topology {
    /// Load topology from JSON file
    pub fn from_json(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let topology: Topology = serde_json::from_str(&content)?;
        Ok(topology)
    }

    /// Compute execution order from device dependencies (topological sort)
    /// For now, we just use the provided order from JSON
    pub fn compute_execution_order(
        &self,
        _devices: &HashMap<String, crate::simulator::devices::Device>,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        // TODO: Implement actual topological sort based on input_ports
        // For now, trust the order provided in topology.json
        Ok(self.execution_order.clone())
    }
}
