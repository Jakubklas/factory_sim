use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::models::DeviceFieldValue;

/// Trait for device control functions that can be called remotely
pub trait DeviceFunction: Send + Sync {
    fn execute(&self, device: &mut super::devices::Device, args: Vec<DeviceFieldValue>) -> Result<(), String>;
}

/// Configuration for device functions - loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeviceFunctionConfig {
    SetField {
        field: String,
        value: serde_json::Value,  // JSON value (float/string)
    },
    SetFieldFromArg {
        field: String,
        arg_index: usize,
    },
    IncrementField {
        field: String,
        amount: f64,
    },
}

/// Factory function to create device functions from config
pub fn create_device_function(config: DeviceFunctionConfig) -> Arc<dyn DeviceFunction> {
    match config {
        DeviceFunctionConfig::SetField { field, value } => {
            // Convert JSON value to DeviceFieldValue
            let device_value = match value {
                serde_json::Value::Number(n) => {
                    if let Some(f) = n.as_f64() {
                        DeviceFieldValue::Float(f)
                    } else {
                        DeviceFieldValue::Float(0.0)
                    }
                }
                serde_json::Value::String(s) => DeviceFieldValue::String(s),
                _ => DeviceFieldValue::String("Unknown".to_string()),
            };
            Arc::new(SetFieldFunction { field, value: device_value })
        }
        DeviceFunctionConfig::SetFieldFromArg { field, arg_index } => {
            Arc::new(SetFieldFromArgFunction { field, arg_index })
        }
        DeviceFunctionConfig::IncrementField { field, amount } => {
            Arc::new(IncrementFieldFunction { field, amount })
        }
    }
}

// ============================================================================
// Set Field Function (sets field to fixed value)
// ============================================================================

struct SetFieldFunction {
    field: String,
    value: DeviceFieldValue,
}

impl DeviceFunction for SetFieldFunction {
    fn execute(&self, device: &mut super::devices::Device, _args: Vec<DeviceFieldValue>) -> Result<(), String> {
        device.set_field(self.field.clone(), self.value.clone());
        Ok(())
    }
}

// ============================================================================
// Set Field From Arg Function (sets field from argument)
// ============================================================================

struct SetFieldFromArgFunction {
    field: String,
    arg_index: usize,
}

impl DeviceFunction for SetFieldFromArgFunction {
    fn execute(&self, device: &mut super::devices::Device, args: Vec<DeviceFieldValue>) -> Result<(), String> {
        let value = args.get(self.arg_index)
            .ok_or_else(|| format!("Missing argument at index {}", self.arg_index))?;
        device.set_field(self.field.clone(), value.clone());
        Ok(())
    }
}

// ============================================================================
// Increment Field Function (increments numeric field)
// ============================================================================

struct IncrementFieldFunction {
    field: String,
    amount: f64,
}

impl DeviceFunction for IncrementFieldFunction {
    fn execute(&self, device: &mut super::devices::Device, _args: Vec<DeviceFieldValue>) -> Result<(), String> {
        let current = device.get_float(&self.field).unwrap_or(0.0);
        let new_value = current + self.amount;
        device.set_field(self.field.clone(), DeviceFieldValue::Float(new_value));
        Ok(())
    }
}
