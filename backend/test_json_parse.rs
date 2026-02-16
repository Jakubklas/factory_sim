// Test JSON parsing for devices.json
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeviceFieldValue {
    Float(f64),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    pub initial_values: HashMap<String, DeviceFieldValue>,
}

fn main() {
    let json = r#"{
        "initial_values": {
            "temperature": 20.0,
            "target_temperature": 85.0,
            "pressure": 0.0,
            "status": "Off"
        }
    }"#;

    match serde_json::from_str::<TestConfig>(json) {
        Ok(config) => {
            println!("✓ Successfully parsed!");
            println!("Config: {:#?}", config);
        }
        Err(e) => {
            println!("✗ Parse error: {}", e);
        }
    }
}
