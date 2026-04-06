use std::collections::HashMap;
use super::enums::{DataType, DeviceCategory, FunctionKind, PhysicsKind};

// ============================================================================
// Device type registry  (loaded from device_types.json)
// ============================================================================

/// Definition of a device type — the "class" that instances reference.
/// Loaded once at startup from device_types.json.
pub struct DeviceTypeDefinition {
    pub device_type: String,                    // "Valve", "Boiler" — the lookup key
    pub category: DeviceCategory,
    pub physics: PhysicsKind,                   // which physics model to use
    pub functions: Vec<DeviceFunctionConfig>,   // control commands available on this type
    pub metrics: Vec<DeviceMetric>,             // field schema + default initial values
    pub required_params: Vec<ParamSpec>,        // params the instance must supply (e.g. target_pressure)
}

/// A single param that an instance must provide for the physics model.
pub struct ParamSpec {
    pub name: String,           // "target_pressure"
    pub description: String,
    pub default: Option<f64>,   // None = truly mandatory; Some = has a sensible default
}

// ============================================================================
// Plant → PLC → Device hierarchy  (loaded from factory.json)
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
pub struct PlcConfig {
    pub plc_id: String,
    pub name: String,
    pub uri: String,
    pub port: u16,
    pub endpoint: String,
    pub devices: Vec<DeviceConfig>,
}

/// A single device instance attached to a PLC.
/// Minimal — just wiring, identity, and instance-specific param values.
/// Physics, functions, and metrics are resolved from DeviceTypeDefinition at load time.
pub struct DeviceConfig {
    pub device_id: String,
    pub name: String,
    pub device_type: String,                // FK into DeviceTypeDefinition registry
    pub input_ports: Vec<InputPort>,
    pub output_ports: Vec<OutputPort>,
    pub tick_ms: Option<u64>,              // overrides PlantConfig.default_tick_ms if set
    pub params: HashMap<String, f64>,      // must satisfy DeviceTypeDefinition.required_params
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

/// A named control function exposed by a device type (e.g. "open", "set_position").
pub struct DeviceFunctionConfig {
    pub name: String,
    pub description: String,
    pub kind: FunctionKind,
}

/// A single observable metric/field on a device type.
/// Acts as both schema (data_type) and seed for runtime state (initial_value).
pub struct DeviceMetric {
    pub name: String,
    pub description: String,
    pub data_type: DataType,
    pub initial_value: Option<DataType>,  // None → DataType default (0.0 / "" / false)
}
