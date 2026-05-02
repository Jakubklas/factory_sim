use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::primitives::{DataType, FunctionKind, PhysicsMode};

// ============================================================================
// Device type registry  (loaded from device_types.json)
// ============================================================================

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceTypeDefinition {
    pub device_type:        String,
    pub physics_mode:       PhysicsMode,
    pub physics_definition: Option<String>,
    pub functions:          Vec<DeviceFunctionConfig>,
    pub metrics:            Vec<DeviceMetric>,
    pub required_params:    Vec<ParamSpec>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ParamSpec {
    pub name:        String,
    pub description: String,
    pub default:     Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceFunctionConfig {
    pub name:        String,
    pub description: String,
    pub kind:        FunctionKind,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceMetric {
    pub name:          String,
    pub description:   String,
    pub data_type:     DataType,
    pub initial_value: Option<DataType>,
}

// ============================================================================
// Plant → PLC → Device hierarchy  (loaded from factory.json)
// ============================================================================

#[derive(Deserialize, Serialize, Debug)]
pub struct PlantConfig {
    pub plant_id:        String,
    pub name:            String,
    pub description:     String,
    pub default_tick_ms: u64,
    pub plcs:            Vec<PlcConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PlcConfig {
    pub plc_id:   String,
    pub name:     String,
    pub protocol: String,  // "opcua" | "modbus"
    pub uri:      String,
    pub port:     u16,
    pub endpoint: String,
    pub devices:  Vec<DeviceConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceConfig {
    pub device_id:       String,
    pub name:            String,
    pub device_type:     String,
    pub input_variables: Vec<InputVariable>,
    pub tick_ms:         Option<u64>,
    pub params:          HashMap<String, f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct InputVariable {
    pub name:             String,
    pub source_device_id: String,
    pub source_field:     String,
}

// ============================================================================
// Connector contract — produced by PlantConfigHandle::endpoint_configs(),
// consumed by the comms layer. Defines what each connector needs to poll.
// ============================================================================

pub struct PlcEndpointConfig {
    pub name:       String,
    pub protocol:   String,  // "opcua" | "modbus"
    pub url:        String,
    pub node_reads: Vec<NodeReadConfig>,
}

pub struct NodeReadConfig {
    pub device_id:   String,
    pub metric_name: String,
    pub node_id:     String,   // "ns=2;s={plc}.{device}.{metric}"
    pub data_type:   DataType,
}
