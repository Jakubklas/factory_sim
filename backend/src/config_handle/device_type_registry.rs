use crate::config_handle::DeviceTypeDefinition;

/// Loads device_types.json.
/// Pure file I/O — no runtime state, no simulation concerns.
/// Hand the result to PlantConfigHandle::new() at startup.
pub struct DeviceTypeRegistry {
    types: Vec<DeviceTypeDefinition>,
}

impl DeviceTypeRegistry {
    /// Load all device type definitions from a JSON file.
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let raw:    serde_json::Value = serde_json::from_str(&content)?;
        let types:  Vec<DeviceTypeDefinition> = serde_json::from_value(
            raw["device_types"].clone()
        )?;
        Ok(Self { types })
    }

    /// Consume the registry and return the type definitions.
    /// Called by PlantConfigHandle::new() — registry is not needed after this.
    pub fn into_types(self) -> Vec<DeviceTypeDefinition> {
        self.types
    }
}
