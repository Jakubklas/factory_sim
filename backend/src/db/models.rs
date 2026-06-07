use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DeployNode {
    pub name:               String,
    pub display_name:       String,
    pub k8s_node:           Option<String>,
    pub arch:               String,
    pub mem_allocatable_mb: Option<i32>,
    pub status:             String,
    pub last_seen:          Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DeviceType {
    /// Natural primary key — also how every other layer identifies a type
    /// (io_spec.device_type, the physics registry, plant.json, the simulator).
    pub name:         String,
    pub physics_rhai: String,
    /// Stores the full DeviceTypeDefinition JSON for round-trip reconstruction.
    pub io_spec:      JsonValue,
    pub model_3d_ref: Option<String>,
    pub icon_ref:     Option<String>,
    pub updated_at:   DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "plc_kind", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PlcKind {
    Simulated,
    Real,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Plc {
    pub id:           Uuid,
    /// Human slug like "plc-001" — used as k8s deploy name and OPC-UA hostname.
    pub plc_id:       Option<String>,
    pub name:         String,
    pub kind:         PlcKind,
    pub deploy_node:  Option<String>,
    pub endpoint_uri: Option<String>,
    pub port:         Option<i32>,
    pub protocol:     String,
    pub endpoint:     String,
    pub updated_at:   DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DeviceInstance {
    pub id:               Uuid,
    pub plc_id:           Uuid,
    pub device_type_name: String,
    /// Short slug like "boiler_001" — used in OPC-UA node paths and IngestedState keys.
    pub device_id:        Option<String>,
    pub name:           String,
    pub param_values:   JsonValue,
    pub floor_pos:      Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Wire {
    pub id:              Uuid,
    pub src_plc_id:      Uuid,
    /// Source device slug (e.g. "boiler_001") — for IngestedState lookup.
    pub src_device_id:   Option<String>,
    /// Source metric name (e.g. "pressure").
    pub src_field:       Option<String>,
    pub dst_instance_id: Uuid,
    pub dst_input_port:  String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DiscoveredNode {
    pub plc_id:       Uuid,
    pub node_id:      String,
    pub browse_path:  Option<String>,
    pub browse_name:  String,
    pub datatype:     Option<String>,
    pub access_level: i32,
    pub last_seen:    DateTime<Utc>,
}

// ============================================================================
// Request / response DTOs
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateDeviceType {
    pub name:         String,
    pub physics_rhai: Option<String>,
    pub io_spec:      Option<JsonValue>,
    pub model_3d_ref: Option<String>,
    pub icon_ref:     Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlc {
    /// Human slug (e.g. "plc-003"). Auto-derived from name if omitted.
    pub plc_id:       Option<String>,
    pub name:         String,
    pub kind:         PlcKind,
    pub deploy_node:  Option<String>,
    pub endpoint_uri: Option<String>,
    pub port:         Option<i32>,
    pub protocol:     Option<String>,
    pub endpoint:     Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDeviceInstance {
    pub plc_id:           Uuid,
    pub device_type_name: String,
    /// Short slug like "pump_002". Auto-derived from name if omitted.
    pub device_id:        Option<String>,
    pub name:             String,
    pub param_values:     Option<JsonValue>,
    pub floor_pos:        Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWire {
    pub src_plc_id:      Uuid,
    pub src_device_id:   String,
    pub src_field:       String,
    pub dst_instance_id: Uuid,
    pub dst_input_port:  String,
}
