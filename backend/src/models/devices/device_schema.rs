use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::DataType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFieldSchema {
    pub name: String,
    pub data_type: DataType,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSchema {
    pub device_type: String,
    pub fields: Vec<DeviceFieldSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSchemaRegistry {
    pub device_schemas: Vec<DeviceSchema>,
}

impl DeviceSchemaRegistry {
    /// Load device schemas from JSON file
    pub fn from_json(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let registry: DeviceSchemaRegistry = serde_json::from_str(&content)?;
        Ok(registry)
    }

    /// Get schema for a specific device type
    pub fn get_schema(&self, device_type: &str) -> Option<&DeviceSchema> {
        self.device_schemas.iter().find(|s| s.device_type == device_type)
    }

    /// Get all available field names for a device type
    pub fn get_field_names(&self, device_type: &str) -> Vec<String> {
        self.get_schema(device_type)
            .map(|schema| schema.fields.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Build a lookup map for quick field access
    pub fn build_field_map(&self, device_type: &str) -> HashMap<String, DataType> {
        self.get_schema(device_type)
            .map(|schema| {
                schema.fields.iter()
                    .map(|f| (f.name.clone(), f.data_type.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Validate that a device mapping has valid fields
    pub fn validate_device_fields(&self, device_type: &str, field_names: &[String]) -> Result<(), String> {
        let schema = self.get_schema(device_type)
            .ok_or_else(|| format!("Unknown device type: {}", device_type))?;

        let valid_fields: Vec<_> = schema.fields.iter().map(|f| f.name.as_str()).collect();

        for field in field_names {
            if !valid_fields.contains(&field.as_str()) {
                return Err(format!(
                    "Invalid field '{}' for device type '{}'. Available fields: {:?}",
                    field, device_type, valid_fields
                ));
            }
        }

        Ok(())
    }
}
