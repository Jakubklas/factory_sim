use std::sync::Arc;
use tokio::sync::RwLock;
use crate::simulator::devices::Device;

/// Represents a device field value
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum DeviceFieldValue {
    Float(f64),
    String(String),
}

/// Generic device handle - wraps any device in an Arc<RwLock>
#[derive(Clone)]
pub struct DeviceHandle {
    device: Arc<RwLock<Device>>,
}

impl DeviceHandle {
    pub fn new(device: Arc<RwLock<Device>>) -> Self {
        Self { device }
    }

    pub async fn read_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        let device = self.device.read().await;
        device.get_field(field_name)
    }

    pub fn get_device(&self) -> Arc<RwLock<Device>> {
        self.device.clone()
    }

    pub async fn call_function(&self, name: &str, args: Vec<DeviceFieldValue>) -> Result<(), String> {
        let mut device = self.device.write().await;
        device.call_function(name, args)
    }
}
