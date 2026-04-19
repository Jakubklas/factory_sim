use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use super::enums::{DataType, FunctionKind, PhysicsMode};

// ============================================================================
// Device type registry  (loaded from device_types.json)
// ============================================================================

/// Definition of a device type — the "class" that instances reference.
/// Loaded once at startup from device_types.json.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceTypeDefinition {
    pub device_type:        String,                    // "Valve", "Boiler" — the lookup key
    pub physics_mode:       PhysicsMode,               // Simulation or Live
    pub physics_definition: Option<String>,            // Rhai script; None = no simulation (Live only)
    pub functions:          Vec<DeviceFunctionConfig>, // control commands available on this type
    pub metrics:            Vec<DeviceMetric>,          // field schema + default initial values
    pub required_params:    Vec<ParamSpec>,             // params the instance must supply
}

/// A single param that an instance must provide for the physics model.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ParamSpec {
    pub name:        String,
    pub description: String,
    pub default:     Option<f64>,  // None = truly mandatory; Some = has a sensible default
}

/// A named control function exposed by a device type (e.g. "open", "set_position").
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceFunctionConfig {
    pub name:        String,
    pub description: String,
    pub kind:        FunctionKind,
}

/// A single observable metric/field on a device type.
/// Acts as both schema (data_type) and seed for runtime state (initial_value).
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceMetric {
    pub name:          String,
    pub description:   String,
    pub data_type:     DataType,           // type marker — inner value is ignored
    pub initial_value: Option<DataType>,   // None → DataType default (0.0 / "" / false)
}

// ============================================================================
// Plant → PLC → Device hierarchy  (loaded from factory.json)
// ============================================================================

/// Top-level config. One plant = one factory / simulation instance.
#[derive(Deserialize, Serialize, Debug)]
pub struct PlantConfig {
    pub plant_id:        String,
    pub name:            String,
    pub description:     String,
    pub default_tick_ms: u64,   // fallback tick interval in ms for all devices
    pub plcs:            Vec<PlcConfig>,
}

/// A PLC (Programmable Logic Controller) — the physical OPC-UA endpoint.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PlcConfig {
    pub plc_id:   String,
    pub name:     String,
    pub protocol: String,  // "opcua" | "modbus" — selects which ConnectorImpl to instantiate
    pub uri:      String,
    pub port:     u16,
    pub endpoint: String,
    pub devices:  Vec<DeviceConfig>,
}

/// A single device instance attached to a PLC.
/// Minimal — just wiring, identity, and instance-specific param values.
/// Physics, functions, and metrics are resolved from DeviceTypeDefinition at load time.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceConfig {
    pub device_id:       String,
    pub name:            String,
    pub device_type:     String,                 // FK into DeviceTypeDefinition registry
    pub input_variables: Vec<InputVariable>,
    pub tick_ms:         Option<u64>,            // overrides PlantConfig.default_tick_ms if set
    pub params:          HashMap<String, f64>,   // must satisfy DeviceTypeDefinition.required_params
}

// ============================================================================
// Device sub-structs
// ============================================================================

/// Defines where a device reads an input value from (another device's field in LiveState).
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct InputVariable {
    pub name:             String,
    pub source_device_id: String,
    pub source_field:     String,
}

// ============================================================================
// Connector config — derived from PlantConfigHandle; the contract between
// the config layer and the connector layer. Connectors know nothing about
// PlantConfigHandle or physics — they only consume this struct.
// ============================================================================

/// Everything a connector needs to poll one endpoint.
/// Built by PlantConfigHandle::endpoint_configs() from the resolved plant config.
pub struct PlcEndpointConfig {
    pub name:       String,  // human-readable label used in logs
    pub protocol:   String,  // "opcua" | "modbus" — determines which ConnectorImpl is used
    pub url:        String,  // full endpoint URL: e.g. "opc.tcp://{host}:{port}{path}"
    pub node_reads: Vec<NodeReadConfig>,
}

/// Spec for a single node the connector will poll each tick.
pub struct NodeReadConfig {
    pub device_id:   String,
    pub metric_name: String,
    pub node_id:     String,   // "ns=2;s={plc}.{device}.{metric}"
    pub data_type:   DataType, // expected variant — used to cast the read result
}
