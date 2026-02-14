use serde::{Deserialize, Serialize};
use crate::models::devices::DeviceMapping;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlcConfig {
    pub name: String,
    pub uri: String,
    pub port: u16,
    pub endpoint: String,
    pub device_mappings: Vec<DeviceMapping>,
}
