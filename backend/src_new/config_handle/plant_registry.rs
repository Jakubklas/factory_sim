use crate::models::PlantConfig;

/// Loads and saves factory.json.
/// Pure file I/O — no runtime state, no simulation concerns.
/// Hand the result to FactoryHandle::new() at startup.
pub struct PlantRegistry {
    path:   String,
    config: PlantConfig,
}

impl PlantRegistry {
    /// Load plant config from a JSON file.
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: PlantConfig = serde_json::from_str(&content)?;
        Ok(Self { path: path.to_string(), config })
    }

    /// Persist current state back to the original JSON file.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(&self.path, serde_json::to_string_pretty(&self.config)?)?;
        Ok(())
    }

    /// Consume the store and return the plant config.
    /// Called by FactoryHandle::new() — store is not needed after this.
    pub fn into_config(self) -> PlantConfig {
        self.config
    }

    pub fn config(&self) -> &PlantConfig {
        &self.config
    }
}
