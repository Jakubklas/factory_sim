use super::enums::{DataType, DeviceCategory, DeviceType, FunctionKind, PhysicsKind};

// ============================================================================
// Plant → PLC → Device hierarchy
// ============================================================================

/// Top-level config. One plant = one factory / simulation instance.
pub struct PlantConfig {
    pub plant_id: String,
    pub name: String,
    pub description: String,
    pub default_tick_ms: u64,   // fallback tick interval in ms for all devices
    pub plcs: Vec<PlcConfig>,
}

/// A PLC (Programmable Logic Controller) — the physical OPC-UA endpoint.
/// Owns a list of devices wired into it.
pub struct PlcConfig {
    pub plc_id: String,
    pub name: String,
    pub uri: String,
    pub port: u16,
    pub endpoint: String,
    pub devices: Vec<DeviceConfig>,
}

/// A single device instance attached to a PLC.
/// Contains everything needed to build the runtime Device in simulator/.
pub struct DeviceConfig {
    pub device_id: String,
    pub name: String,
    pub device_type: DeviceType,
    pub category: DeviceCategory,
    pub input_ports: Vec<InputPort>,
    pub output_ports: Vec<OutputPort>,
    pub physics: PhysicsKind,
    pub functions: Vec<DeviceFunctionConfig>,
    pub metrics: Vec<DeviceMetric>,
    pub tick_ms: Option<u64>,   // overrides PlantConfig.default_tick_ms if set
}

// ============================================================================
// Device sub-structs
// ============================================================================

/// Defines where a device reads an input value from (another device's field).
pub struct InputPort {
    pub name: String,
    pub source_device_id: String,
    pub source_field: String,
}

/// Defines a field this device writes to (consumed by downstream devices).
pub struct OutputPort {
    pub name: String,
    pub target_field: String,
}

/// A named control function exposed by this device (e.g. "open", "set_position").
/// Named DeviceFunctionConfig (not DeviceFunction) to avoid clashing
/// with the DeviceFunction trait that will live in simulator/.
pub struct DeviceFunctionConfig {
    pub name: String,
    pub description: String,
    pub kind: FunctionKind,
}

/// A single observable metric/field on a device.
/// Acts as both schema (data_type declares what type) and
/// seed for runtime state (initial_value seeds Device.fields at startup).
pub struct DeviceMetric {
    pub name: String,
    pub description: String,
    pub data_type: DataType,
    pub initial_value: Option<DataType>,
}
