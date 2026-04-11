use crate::models::DeviceTypeDefinition;

/// Loads and saves device_types.json.
/// Pure file I/O — no runtime state, no simulation concerns.
/// Hand the result to FactoryHandle::new() at startup.
pub struct DeviceTypeRegistry {
    path:  String,
    types: Vec<DeviceTypeDefinition>,
}

impl DeviceTypeRegistry {
    /// Load all device type definitions from a JSON file.
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content  = std::fs::read_to_string(path)?;
        let raw:     serde_json::Value = serde_json::from_str(&content)?;
        let types:   Vec<DeviceTypeDefinition> = serde_json::from_value(
            raw["device_types"].clone()
        )?;

        Ok(Self { path: path.to_string(), types })
    }

    /// Add a new device type. Errors if the type already exists.
    pub fn register(&mut self, def: DeviceTypeDefinition) -> Result<(), String> {
        if self.types.iter().any(|t| t.device_type == def.device_type) {
            return Err(format!("Device type '{}' already exists", def.device_type));
        }
        self.types.push(def);
        Ok(())
    }

    /// Remove a device type by name. Errors if not found.
    /// Note: caller is responsible for ensuring no plant instances reference this type.
    pub fn remove(&mut self, device_type: &str) -> Result<(), String> {
        let pos = self.types.iter().position(|t| t.device_type == device_type)
            .ok_or_else(|| format!("Device type '{}' not found", device_type))?;
        self.types.remove(pos);
        Ok(())
    }

    /// Persist current state back to the original JSON file.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let payload = serde_json::json!({ "device_types": self.types });
        std::fs::write(&self.path, serde_json::to_string_pretty(&payload)?)?;
        Ok(())
    }

    /// Consume the registry and return the type definitions.
    /// Called by FactoryHandle::new() — registry is not needed after this.
    pub fn into_types(self) -> Vec<DeviceTypeDefinition> {
        self.types
    }

    pub fn all(&self) -> &[DeviceTypeDefinition] {
        &self.types
    }
}
