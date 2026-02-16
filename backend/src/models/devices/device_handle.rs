use std::sync::Arc;
use tokio::sync::RwLock;
use crate::simulator::devices::{Boiler, PressureMeter, FlowMeter, Valve, DeviceFields, Device};

// OLD DEVICE HANDLE (will be removed in Phase 9)
#[allow(dead_code)]
pub enum OldDeviceHandle {
    Boiler(Arc<RwLock<Boiler>>),
    PressureMeter(Arc<RwLock<PressureMeter>>),
    FlowMeter(Arc<RwLock<FlowMeter>>),
    Valve(Arc<RwLock<Valve>>),
}

/// Represents a device field value that can be converted to OPC UA Variant
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DeviceFieldValue {
    Float(f64),
    String(String),
}

impl OldDeviceHandle {
    /// Generic method to read a field value from any device type.
    /// Uses the DeviceFields trait implemented by each device.
    /// Returns None if the field doesn't exist for this device type.
    #[allow(dead_code)]
    pub async fn read_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        match self {
            OldDeviceHandle::Boiler(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
            OldDeviceHandle::PressureMeter(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
            OldDeviceHandle::FlowMeter(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
            OldDeviceHandle::Valve(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
        }
    }
}

// ============================================================================
// NEW DEVICE HANDLE (Generic Wrapper)
// ============================================================================

/// Generic device handle that works with any device type
#[derive(Clone)]
pub struct DeviceHandle {
    device: Arc<RwLock<Device>>,
}

impl DeviceHandle {
    /// Create a new device handle
    pub fn new(device: Arc<RwLock<Device>>) -> Self {
        Self { device }
    }

    /// Read a field value from the device
    pub async fn read_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        let device = self.device.read().await;
        device.get_field(field_name)
    }

    /// Get the underlying device Arc for direct access
    pub fn get_device(&self) -> Arc<RwLock<Device>> {
        self.device.clone()
    }

    /// Call a function on the device
    pub async fn call_function(
        &self,
        name: &str,
        args: Vec<DeviceFieldValue>,
    ) -> Result<(), String> {
        let mut device = self.device.write().await;
        device.call_function(name, args)
    }
}
