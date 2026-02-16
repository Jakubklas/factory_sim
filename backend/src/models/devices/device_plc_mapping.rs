use serde::{Deserialize, Serialize};
use super::metrics::MetricConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMapping {
    pub device_id: String,
    /// Device type as string - must match a device_type in available_devices.json
    pub device_type: String,
    pub folder_name: String,
    pub metrics: Vec<MetricConfig>,
}

