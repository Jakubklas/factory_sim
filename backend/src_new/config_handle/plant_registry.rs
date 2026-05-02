use crate::config_handle::PlantConfig;

/// Loads factory.json.
/// Pure file I/O — no runtime state, no simulation concerns.
/// Hand the result to PlantConfigHandle::new() at startup.
pub struct PlantRegistry {
    config: PlantConfig,
}

impl PlantRegistry {
    /// Load plant config from a JSON file.
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: PlantConfig = serde_json::from_str(&content)?;
        Ok(Self { config })
    }

    /// Consume the store and return the plant config.
    /// Called by PlantConfigHandle::new() — store is not needed after this.
    pub fn into_config(self) -> PlantConfig {
        self.config
    }
}
