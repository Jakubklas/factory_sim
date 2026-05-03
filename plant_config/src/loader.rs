use crate::schema::{PlantConfig, DeviceTypeDefinition};

/// Load plant config from a JSON file (factory.json).
pub fn load_plant_config(path: &str) -> Result<PlantConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

/// Load all device type definitions from a JSON file (device_types.json).
pub fn load_device_types(path: &str) -> Result<Vec<DeviceTypeDefinition>, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let raw: serde_json::Value = serde_json::from_str(&content)?;
    Ok(serde_json::from_value(raw["device_types"].clone())?)
}
