use sqlx::PgPool;
use serde_json::json;

use super::models::{DeployNode, PlcKind};
use super::queries::{upsert_deploy_node, upsert_device_type_by_name, upsert_plc, upsert_device_instance};

// Embedded JSON blobs so the backend binary is self-contained. The seeder is
// idempotent (ON CONFLICT … DO UPDATE) so it can run on every startup.
const DEVICE_TYPES_JSON: &str = include_str!("../../../config/device_types.json");
const PLANT_JSON:        &str = include_str!("../../../config/plant.json");
const INVENTORY_JSON:    &str = include_str!("../../../deploy/inventory.json");

pub async fn seed_from_configs(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    seed_deploy_nodes(pool).await?;
    seed_device_types(pool).await?;
    seed_plcs_and_instances(pool).await?;
    tracing::info!("DB seed complete");
    Ok(())
}

async fn seed_deploy_nodes(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct InventoryFile { devices: std::collections::HashMap<String, InventoryEntry> }
    #[derive(serde::Deserialize)]
    struct InventoryEntry { name: String, architecture: String }

    let inv: InventoryFile = serde_json::from_str(INVENTORY_JSON)?;
    for (node_name, entry) in &inv.devices {
        let node = DeployNode {
            name:               node_name.clone(),
            display_name:       entry.name.clone(),
            k8s_node:           None,
            arch:               entry.architecture.clone(),
            mem_allocatable_mb: None,
            status:             "unknown".into(),
            last_seen:          None,
        };
        upsert_deploy_node(pool, &node).await?;
        tracing::debug!("Seeded deploy_node: {}", node_name);
    }
    Ok(())
}

async fn seed_device_types(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct DeviceTypesFile { device_types: Vec<RawDeviceType> }
    #[derive(serde::Deserialize)]
    struct RawDeviceType {
        device_type:        String,
        physics_definition: String,
        metrics:            Vec<serde_json::Value>,
        required_params:    Vec<serde_json::Value>,
    }

    let file: DeviceTypesFile = serde_json::from_str(DEVICE_TYPES_JSON)?;
    for dt in &file.device_types {
        // Build io_spec from legacy metrics + required_params.
        // All metrics are treated as outputs for now; inputs come from wiring (Phase 4).
        let io_spec = json!({
            "outputs": dt.metrics.iter().map(|m| json!({
                "name":    m["name"],
                "type":    m["data_type"],
                "initial": m.get("initial_value").cloned().unwrap_or(serde_json::Value::Null)
            })).collect::<Vec<_>>(),
            "params": dt.required_params.iter().map(|p| json!({
                "name":    p["name"],
                "default": p.get("default").cloned().unwrap_or(serde_json::Value::Null)
            })).collect::<Vec<_>>(),
            "inputs": []
        });

        upsert_device_type_by_name(pool, &dt.device_type, &dt.physics_definition, io_spec).await?;
        tracing::debug!("Seeded device_type: {}", dt.device_type);
    }
    Ok(())
}

async fn seed_plcs_and_instances(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct PlantFile { plcs: Vec<RawPlc> }
    #[derive(serde::Deserialize)]
    struct RawPlc {
        plc_id:       String,
        name:         String,
        simulated:    bool,
        port:         Option<u16>,
        deploy_target: Option<String>,
        uri:          Option<String>,
        devices:      Vec<RawDevice>,
    }
    #[derive(serde::Deserialize)]
    struct RawDevice {
        device_id:   String,
        name:        String,
        device_type: String,
        params:      serde_json::Value,
    }

    let plant: PlantFile = serde_json::from_str(PLANT_JSON)?;

    for raw_plc in &plant.plcs {
        let kind = if raw_plc.simulated { PlcKind::Simulated } else { PlcKind::Real };
        let deploy_node  = raw_plc.deploy_target.as_deref();
        let endpoint_uri = raw_plc.uri.as_deref();
        let port = raw_plc.port.map(|p| p as i32);

        let plc = upsert_plc(pool, &raw_plc.name, kind, deploy_node, endpoint_uri, port).await?;
        tracing::debug!("Seeded plc: {} ({})", raw_plc.name, plc.id);

        for raw_dev in &raw_plc.devices {
            // Look up device_type by name to get its ID.
            let dt = super::queries::get_device_type_by_name(pool, &raw_dev.device_type).await?;

            let Some(dt) = dt else {
                tracing::warn!(
                    "Device '{}' references unknown device_type '{}' — skipping",
                    raw_dev.name, raw_dev.device_type
                );
                continue;
            };

            upsert_device_instance(pool, plc.id, dt.id, &raw_dev.name, raw_dev.params.clone()).await?;
            tracing::debug!("Seeded instance: {}.{}", raw_plc.name, raw_dev.name);
        }
    }
    Ok(())
}
