use std::sync::Arc;
use tokio::sync::RwLock;
use crate::simulator::devices::{Boiler, PressureMeter, FlowMeter, Valve, DeviceFields};

/// Central type-safe handle for accessing device instances across the architecture.
/// This enum provides a unified interface for different device types while maintaining type safety.
pub enum DeviceHandle {
    Boiler(Arc<RwLock<Boiler>>),
    PressureMeter(Arc<RwLock<PressureMeter>>),
    FlowMeter(Arc<RwLock<FlowMeter>>),
    Valve(Arc<RwLock<Valve>>),
}

/// Represents a device field value that can be converted to OPC UA Variant
pub enum DeviceFieldValue {
    Float(f64),
    String(String),
}

impl DeviceHandle {
    /// Generic method to read a field value from any device type.
    /// Uses the DeviceFields trait implemented by each device.
    /// Returns None if the field doesn't exist for this device type.
    pub async fn read_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        match self {
            DeviceHandle::Boiler(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
            DeviceHandle::PressureMeter(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
            DeviceHandle::FlowMeter(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
            DeviceHandle::Valve(device) => {
                let data = device.read().await;
                data.get_field(field_name)
            }
        }
    }
}
