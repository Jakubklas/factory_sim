use serde::{Deserialize, Serialize};
use crate::models::plc::PlcConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlantConfig {
    pub name: String,
    pub plcs: Vec<PlcConfig>,
}

impl PlantConfig {
    pub fn from_json(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: PlantConfig = serde_json::from_str(&content)?;
        Ok(config)
    }
}
