
// STRUCTS

pub struct PlantConfig {
    pub plant_id: String,
    pub name: String,
    pub description: String,
    pub plcs: Vec<PlcConfig>,
}

pub struct PlcConfig {
    pub plc_id: String,
    pub name: String,
    pub uri: String,
    pub port: String,
    pub enpoint: String,
    pub devices: Vec<DeviceConfig>,
}

pub struct DeviceConfig {
    pub device_id: String,
    pub name: String,
    pub device_type: DeviceType,
    pub functions: Vec<DeviceFunction>,
    pub pyhsics: PysicsConfig,
    pub state: Vec<DeviceMetric>,
}

pub struct DeviceFunction {
    pub name: String,
    pub description: String,
    // DeviceFunction trait separate from the data model
}

pub struct DeviceMetric {
    pub name: String,
    pub description: String,
    pub data_type: DataType,
    pub initial_value: Option<DataType>,
}

// ENUMS

pub enum DeviceType {
    Pump,
    Boiler,
    Valve,
}

pub enum DataType {
    Float(f64),
    String(String),
    Boolean(Boolean),
}