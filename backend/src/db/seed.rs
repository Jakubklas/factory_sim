/// Optional seeder — runs only when SEED_DIR is set AND the DB is empty.
///
/// SEED_DIR should point to a directory containing:
///   device_types.json  — array of DeviceTypeDefinition objects
///   plant.json         — { plcs: [PlcConfig, ...] }
///   inventory.json     — (optional) { devices: { "<name>": { name, architecture } } }
///
/// If SEED_DIR is not set, or the DB already has data, this is a no-op.
/// At runtime the backend always reads from the DB — JSON files are never consulted again.
use std::path::Path;
use sqlx::PgPool;
use plant_config::DeviceTypeDefinition;

use super::models::{DeployNode, PlcKind};
use super::queries::{
    upsert_deploy_node, upsert_device_type, upsert_plc, upsert_device_instance,
};

pub async fn seed_if_configured(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let seed_dir = match std::env::var("SEED_DIR") {
        Ok(d) if !d.is_empty() => d,
        _ => {
            tracing::debug!("[seed] SEED_DIR not set — skipping seed");
            return Ok(());
        }
    };

    // Only seed if the DB is completely empty.
    let plc_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM plcs")
        .fetch_one(pool).await?;
    let dt_count: (i64,)  = sqlx::query_as("SELECT COUNT(*) FROM device_types")
        .fetch_one(pool).await?;

    if plc_count.0 > 0 || dt_count.0 > 0 {
        tracing::info!("[seed] DB already has data — skipping seed");
        return Ok(());
    }

    tracing::info!("[seed] DB is empty — seeding from {}", seed_dir);
    let dir = Path::new(&seed_dir);

    // Order matters: deploy_nodes must exist before plcs (plcs.deploy_node is a FK
    // into deploy_nodes), and device_types before instances.
    seed_deploy_nodes(pool, dir).await?;
    seed_device_types(pool, dir).await?;
    seed_plcs_and_instances(pool, dir).await?;

    tracing::info!("[seed] Seed complete");
    Ok(())
}

// ============================================================================
// Device types
// ============================================================================

async fn seed_device_types(pool: &PgPool, dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = dir.join("device_types.json");
    if !path.exists() {
        tracing::debug!("[seed] device_types.json not found — skipping");
        return Ok(());
    }

    let raw = std::fs::read_to_string(&path)?;

    // Support both:
    //   { "device_types": [...] }   (legacy wrapper)
    //   [...]                       (bare array)
    let defs: Vec<DeviceTypeDefinition> = {
        let v: serde_json::Value = serde_json::from_str(&raw)?;
        if let Some(arr) = v.get("device_types").and_then(|a| a.as_array()) {
            serde_json::from_value(serde_json::Value::Array(arr.clone()))?
        } else {
            serde_json::from_value(v)?
        }
    };

    for dt in &defs {
        // Store the full DeviceTypeDefinition as io_spec so load_device_types() can round-trip it.
        let io_spec = serde_json::to_value(dt)?;
        let physics = dt.physics_definition.as_deref().unwrap_or("");
        upsert_device_type(pool, &dt.device_type, physics, io_spec).await?;
        tracing::debug!("[seed] device_type: {}", dt.device_type);
    }
    Ok(())
}

// ============================================================================
// PLCs and device instances
// ============================================================================

async fn seed_plcs_and_instances(pool: &PgPool, dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = dir.join("plant.json");
    if !path.exists() {
        tracing::debug!("[seed] plant.json not found — skipping");
        return Ok(());
    }

    let raw = std::fs::read_to_string(&path)?;

    // Accept both a bare PlcConfig array and a { plcs: [...] } wrapper.
    #[derive(serde::Deserialize)]
    struct PlantFile { plcs: Vec<plant_config::PlcConfig> }

    let plcs: Vec<plant_config::PlcConfig> = {
        let v: serde_json::Value = serde_json::from_str(&raw)?;
        if v.get("plcs").is_some() {
            serde_json::from_value::<PlantFile>(v)?.plcs
        } else {
            serde_json::from_value(v)?
        }
    };

    // device_id → owning PLC uuid, and device_id → instance uuid.
    // Built during the instance pass, used afterwards to seed wires (whose source
    // device may live on a different PLC).
    let mut device_to_plc:      std::collections::HashMap<String, uuid::Uuid> = std::collections::HashMap::new();
    let mut device_to_instance: std::collections::HashMap<String, uuid::Uuid> = std::collections::HashMap::new();

    for plc in &plcs {
        let kind = if plc.simulated { PlcKind::Simulated } else { PlcKind::Real };
        let db_plc = upsert_plc(
            pool,
            &plc.plc_id,
            &plc.name,
            kind,
            plc.deploy_target.as_deref(),
            plc.uri.as_deref(),
            Some(plc.port as i32),
            &plc.protocol,
            &plc.endpoint,
        ).await?;
        tracing::debug!("[seed] plc: {} ({})", plc.name, db_plc.id);

        for dev in &plc.devices {
            let dt = super::queries::get_device_type_by_name(pool, &dev.device_type).await?;
            let Some(dt) = dt else {
                tracing::warn!(
                    "[seed] Device '{}' references unknown type '{}' — skipping",
                    dev.name, dev.device_type
                );
                continue;
            };

            let param_values = serde_json::to_value(&dev.params)?;
            let inst = upsert_device_instance(
                pool,
                db_plc.id,
                dt.id,
                &dev.device_id,
                &dev.name,
                param_values,
            ).await?;
            device_to_plc.insert(dev.device_id.clone(), db_plc.id);
            device_to_instance.insert(dev.device_id.clone(), inst.id);
            tracing::debug!("[seed] instance: {}.{}", plc.name, dev.name);
        }
    }

    // Second pass: turn each device's input_variables into wire rows. Done after all
    // instances exist so cross-PLC sources resolve.
    for plc in &plcs {
        for dev in &plc.devices {
            let Some(&dst_instance_id) = device_to_instance.get(&dev.device_id) else { continue };
            for input in &dev.input_variables {
                let Some(&src_plc_id) = device_to_plc.get(&input.source_device_id) else {
                    tracing::warn!(
                        "[seed] Wire for '{}.{}' references unknown source device '{}' — skipping",
                        dev.device_id, input.name, input.source_device_id
                    );
                    continue;
                };
                super::queries::upsert_wire(
                    pool,
                    src_plc_id,
                    &input.source_device_id,
                    &input.source_field,
                    dst_instance_id,
                    &input.name,
                ).await?;
                tracing::debug!(
                    "[seed] wire: {}.{} → {}.{}",
                    input.source_device_id, input.source_field, dev.device_id, input.name
                );
            }
        }
    }
    Ok(())
}

// ============================================================================
// Deploy nodes (inventory.json — optional)
// ============================================================================

async fn seed_deploy_nodes(pool: &PgPool, dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = dir.join("inventory.json");
    if !path.exists() {
        tracing::debug!("[seed] inventory.json not found — skipping");
        return Ok(());
    }

    #[derive(serde::Deserialize)]
    struct InventoryFile { devices: std::collections::HashMap<String, InventoryEntry> }
    #[derive(serde::Deserialize)]
    struct InventoryEntry { name: String, architecture: String }

    let raw  = std::fs::read_to_string(&path)?;
    let inv: InventoryFile = serde_json::from_str(&raw)?;

    for (node_name, entry) in &inv.devices {
        upsert_deploy_node(pool, &DeployNode {
            name:               node_name.clone(),
            display_name:       entry.name.clone(),
            k8s_node:           None,
            arch:               entry.architecture.clone(),
            mem_allocatable_mb: None,
            status:             "unknown".into(),
            last_seen:          None,
        }).await?;
        tracing::debug!("[seed] deploy_node: {}", node_name);
    }
    Ok(())
}
