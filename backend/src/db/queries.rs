#![allow(dead_code)]
// Runtime query API — no macros, so we compile without a live DATABASE_URL.

use sqlx::PgPool;
use uuid::Uuid;
use serde_json::Value;

use super::models::{
    DeployNode, DeviceType, Plc, PlcKind,
    DeviceInstance, Wire, DiscoveredNode,
    CreateDeviceType, CreatePlc, CreateDeviceInstance, CreateWire,
};

// ============================================================================
// deploy_nodes
// ============================================================================

pub async fn list_deploy_nodes(pool: &PgPool) -> sqlx::Result<Vec<DeployNode>> {
    sqlx::query_as::<_, DeployNode>(
        "SELECT name, display_name, k8s_node, arch, mem_allocatable_mb, status, last_seen
         FROM deploy_nodes ORDER BY name"
    ).fetch_all(pool).await
}

pub async fn upsert_deploy_node(pool: &PgPool, node: &DeployNode) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO deploy_nodes (name, display_name, k8s_node, arch, mem_allocatable_mb, status, last_seen)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (name) DO UPDATE SET
           display_name       = EXCLUDED.display_name,
           k8s_node           = EXCLUDED.k8s_node,
           arch               = EXCLUDED.arch,
           mem_allocatable_mb = EXCLUDED.mem_allocatable_mb,
           status             = EXCLUDED.status,
           last_seen          = EXCLUDED.last_seen"
    )
    .bind(&node.name).bind(&node.display_name).bind(&node.k8s_node)
    .bind(&node.arch).bind(node.mem_allocatable_mb).bind(&node.status).bind(node.last_seen)
    .execute(pool).await?;
    Ok(())
}

// ============================================================================
// device_types
// ============================================================================

pub async fn list_device_types(pool: &PgPool) -> sqlx::Result<Vec<DeviceType>> {
    sqlx::query_as::<_, DeviceType>(
        "SELECT id, name, physics_rhai, io_spec, model_ref, icon_ref, updated_at
         FROM device_types ORDER BY name"
    ).fetch_all(pool).await
}

pub async fn get_device_type(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<DeviceType>> {
    sqlx::query_as::<_, DeviceType>(
        "SELECT id, name, physics_rhai, io_spec, model_ref, icon_ref, updated_at
         FROM device_types WHERE id = $1"
    ).bind(id).fetch_optional(pool).await
}

pub async fn get_device_type_by_name(pool: &PgPool, name: &str) -> sqlx::Result<Option<DeviceType>> {
    sqlx::query_as::<_, DeviceType>(
        "SELECT id, name, physics_rhai, io_spec, model_ref, icon_ref, updated_at
         FROM device_types WHERE name = $1"
    ).bind(name).fetch_optional(pool).await
}

pub async fn create_device_type(pool: &PgPool, req: &CreateDeviceType) -> sqlx::Result<DeviceType> {
    sqlx::query_as::<_, DeviceType>(
        "INSERT INTO device_types (name, physics_rhai, io_spec, model_ref, icon_ref)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, physics_rhai, io_spec, model_ref, icon_ref, updated_at"
    )
    .bind(&req.name)
    .bind(req.physics_rhai.as_deref().unwrap_or(""))
    .bind(req.io_spec.as_ref().cloned().unwrap_or_else(|| serde_json::json!({})))
    .bind(&req.model_ref)
    .bind(&req.icon_ref)
    .fetch_one(pool).await
}

pub async fn delete_device_type(pool: &PgPool, id: Uuid) -> sqlx::Result<u64> {
    Ok(sqlx::query("DELETE FROM device_types WHERE id = $1")
        .bind(id).execute(pool).await?.rows_affected())
}

/// Upsert by name. Stores the full DeviceTypeDefinition JSON in io_spec for round-tripping.
pub async fn upsert_device_type(
    pool:         &PgPool,
    name:         &str,
    physics_rhai: &str,
    io_spec:      Value,
) -> sqlx::Result<DeviceType> {
    sqlx::query_as::<_, DeviceType>(
        "INSERT INTO device_types (name, physics_rhai, io_spec)
         VALUES ($1, $2, $3)
         ON CONFLICT (name) DO UPDATE SET
           physics_rhai = EXCLUDED.physics_rhai,
           io_spec      = EXCLUDED.io_spec,
           updated_at   = NOW()
         RETURNING id, name, physics_rhai, io_spec, model_ref, icon_ref, updated_at"
    )
    .bind(name).bind(physics_rhai).bind(io_spec)
    .fetch_one(pool).await
}

// ============================================================================
// plcs
// ============================================================================

pub async fn list_plcs(pool: &PgPool) -> sqlx::Result<Vec<Plc>> {
    sqlx::query_as::<_, Plc>(
        "SELECT id, plc_id, name, kind, deploy_node, endpoint_uri, port, protocol, endpoint, updated_at
         FROM plcs ORDER BY name"
    ).fetch_all(pool).await
}

pub async fn get_plc(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Plc>> {
    sqlx::query_as::<_, Plc>(
        "SELECT id, plc_id, name, kind, deploy_node, endpoint_uri, port, protocol, endpoint, updated_at
         FROM plcs WHERE id = $1"
    ).bind(id).fetch_optional(pool).await
}

pub async fn create_plc(pool: &PgPool, req: &CreatePlc) -> sqlx::Result<Plc> {
    let plc_id = req.plc_id.clone()
        .unwrap_or_else(|| slugify(&req.name));
    sqlx::query_as::<_, Plc>(
        "INSERT INTO plcs (plc_id, name, kind, deploy_node, endpoint_uri, port, protocol, endpoint)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, plc_id, name, kind, deploy_node, endpoint_uri, port, protocol, endpoint, updated_at"
    )
    .bind(&plc_id).bind(&req.name).bind(&req.kind)
    .bind(&req.deploy_node).bind(&req.endpoint_uri).bind(req.port)
    .bind(req.protocol.as_deref().unwrap_or("opcua"))
    .bind(req.endpoint.as_deref().unwrap_or("/"))
    .fetch_one(pool).await
}

pub async fn delete_plc(pool: &PgPool, id: Uuid) -> sqlx::Result<u64> {
    Ok(sqlx::query("DELETE FROM plcs WHERE id = $1")
        .bind(id).execute(pool).await?.rows_affected())
}

pub async fn upsert_plc(
    pool:         &PgPool,
    plc_id_slug:  &str,
    name:         &str,
    kind:         PlcKind,
    deploy_node:  Option<&str>,
    endpoint_uri: Option<&str>,
    port:         Option<i32>,
    protocol:     &str,
    endpoint:     &str,
) -> sqlx::Result<Plc> {
    sqlx::query_as::<_, Plc>(
        "INSERT INTO plcs (plc_id, name, kind, deploy_node, endpoint_uri, port, protocol, endpoint)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (name) DO UPDATE SET
           plc_id       = EXCLUDED.plc_id,
           kind         = EXCLUDED.kind,
           deploy_node  = EXCLUDED.deploy_node,
           endpoint_uri = EXCLUDED.endpoint_uri,
           port         = EXCLUDED.port,
           protocol     = EXCLUDED.protocol,
           endpoint     = EXCLUDED.endpoint,
           updated_at   = NOW()
         RETURNING id, plc_id, name, kind, deploy_node, endpoint_uri, port, protocol, endpoint, updated_at"
    )
    .bind(plc_id_slug).bind(name).bind(&kind)
    .bind(deploy_node).bind(endpoint_uri).bind(port)
    .bind(protocol).bind(endpoint)
    .fetch_one(pool).await
}

// ============================================================================
// device_instances
// ============================================================================

pub async fn list_device_instances_for_plc(pool: &PgPool, plc_id: Uuid) -> sqlx::Result<Vec<DeviceInstance>> {
    sqlx::query_as::<_, DeviceInstance>(
        "SELECT id, plc_id, device_type_id, device_id, name, param_values, floor_pos
         FROM device_instances WHERE plc_id = $1 ORDER BY name"
    ).bind(plc_id).fetch_all(pool).await
}

pub async fn create_device_instance(pool: &PgPool, req: &CreateDeviceInstance) -> sqlx::Result<DeviceInstance> {
    let device_id = req.device_id.clone()
        .unwrap_or_else(|| slugify(&req.name));
    sqlx::query_as::<_, DeviceInstance>(
        "INSERT INTO device_instances (plc_id, device_type_id, device_id, name, param_values, floor_pos)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, plc_id, device_type_id, device_id, name, param_values, floor_pos"
    )
    .bind(req.plc_id).bind(req.device_type_id)
    .bind(&device_id).bind(&req.name)
    .bind(req.param_values.as_ref().cloned().unwrap_or_else(|| serde_json::json!({})))
    .bind(&req.floor_pos)
    .fetch_one(pool).await
}

pub async fn delete_device_instance(pool: &PgPool, id: Uuid) -> sqlx::Result<u64> {
    Ok(sqlx::query("DELETE FROM device_instances WHERE id = $1")
        .bind(id).execute(pool).await?.rows_affected())
}

/// Delete an instance only if it belongs to the given PLC. Returns 0 rows if the
/// instance doesn't exist or is owned by a different PLC (so the URL :plc_id is
/// authoritative and can't be used to reach another PLC's instances).
pub async fn delete_device_instance_for_plc(pool: &PgPool, plc_id: Uuid, id: Uuid) -> sqlx::Result<u64> {
    Ok(sqlx::query("DELETE FROM device_instances WHERE id = $1 AND plc_id = $2")
        .bind(id).bind(plc_id).execute(pool).await?.rows_affected())
}

pub async fn upsert_device_instance(
    pool:           &PgPool,
    plc_uuid:       Uuid,
    device_type_id: Uuid,
    device_id_slug: &str,
    name:           &str,
    param_values:   Value,
) -> sqlx::Result<DeviceInstance> {
    sqlx::query_as::<_, DeviceInstance>(
        "INSERT INTO device_instances (plc_id, device_type_id, device_id, name, param_values)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (plc_id, name) DO UPDATE SET
           device_type_id = EXCLUDED.device_type_id,
           device_id      = EXCLUDED.device_id,
           param_values   = EXCLUDED.param_values
         RETURNING id, plc_id, device_type_id, device_id, name, param_values, floor_pos"
    )
    .bind(plc_uuid).bind(device_type_id)
    .bind(device_id_slug).bind(name).bind(param_values)
    .fetch_one(pool).await
}

// ============================================================================
// wires
// ============================================================================

pub async fn list_wires(pool: &PgPool) -> sqlx::Result<Vec<Wire>> {
    sqlx::query_as::<_, Wire>(
        "SELECT id, src_plc_id, src_device_id, src_field, dst_instance_id, dst_input_port
         FROM wires ORDER BY id"
    ).fetch_all(pool).await
}

pub async fn create_wire(pool: &PgPool, req: &CreateWire) -> sqlx::Result<Wire> {
    sqlx::query_as::<_, Wire>(
        "INSERT INTO wires (src_plc_id, src_device_id, src_field, dst_instance_id, dst_input_port)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, src_plc_id, src_device_id, src_field, dst_instance_id, dst_input_port"
    )
    .bind(req.src_plc_id).bind(&req.src_device_id).bind(&req.src_field)
    .bind(req.dst_instance_id).bind(&req.dst_input_port)
    .fetch_one(pool).await
}

pub async fn delete_wire(pool: &PgPool, id: Uuid) -> sqlx::Result<u64> {
    Ok(sqlx::query("DELETE FROM wires WHERE id = $1")
        .bind(id).execute(pool).await?.rows_affected())
}

/// Insert a wire if one doesn't already target the same (dst_instance_id, dst_input_port).
/// Used by the seeder to import plant.json's input_variables.
pub async fn upsert_wire(
    pool:            &PgPool,
    src_plc_id:      Uuid,
    src_device_id:   &str,
    src_field:       &str,
    dst_instance_id: Uuid,
    dst_input_port:  &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO wires (src_plc_id, src_device_id, src_field, dst_instance_id, dst_input_port)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (dst_instance_id, dst_input_port) DO NOTHING"
    )
    .bind(src_plc_id).bind(src_device_id).bind(src_field)
    .bind(dst_instance_id).bind(dst_input_port)
    .execute(pool).await?;
    Ok(())
}

// ============================================================================
// discovered_nodes
// ============================================================================

pub async fn list_discovered_nodes(pool: &PgPool, plc_id: Uuid) -> sqlx::Result<Vec<DiscoveredNode>> {
    sqlx::query_as::<_, DiscoveredNode>(
        "SELECT plc_id, node_id, browse_path, browse_name, datatype, access_level, last_seen
         FROM discovered_nodes WHERE plc_id = $1 ORDER BY browse_path"
    ).bind(plc_id).fetch_all(pool).await
}

pub async fn upsert_discovered_node(pool: &PgPool, node: &DiscoveredNode) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO discovered_nodes (plc_id, node_id, browse_path, browse_name, datatype, access_level)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (plc_id, node_id) DO UPDATE SET
           browse_path  = EXCLUDED.browse_path,
           browse_name  = EXCLUDED.browse_name,
           datatype     = EXCLUDED.datatype,
           access_level = EXCLUDED.access_level,
           last_seen    = NOW()"
    )
    .bind(node.plc_id).bind(&node.node_id).bind(&node.browse_path)
    .bind(&node.browse_name).bind(&node.datatype).bind(node.access_level)
    .execute(pool).await?;
    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Derive a URL-safe slug from a display name.
/// "Heating PLC" → "heating-plc", "Main Boiler" → "main-boiler"
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
