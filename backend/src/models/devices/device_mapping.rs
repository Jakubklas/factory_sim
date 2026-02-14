use serde::{Deserialize, Serialize};
use super::metrics::MetricConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMapping {
    pub device_id: String,
    pub device_type: DeviceType,
    pub folder_name: String,
    pub metrics: Vec<MetricConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceType {
    Boiler,
    PressureMeter,
    FlowMeter,
    Valve,
}
