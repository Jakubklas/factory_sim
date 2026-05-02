use serde::Deserialize;

/// Application-level runtime config — ports, hosts, tick rate.
/// Separate from plant topology (factory.json) and device physics (device_types.json).
#[derive(Deserialize, Debug)]
pub struct AppConfig {
    pub ws_host: String,
    pub ws_port: u16,
    pub tick_ms: u64,
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }
}
