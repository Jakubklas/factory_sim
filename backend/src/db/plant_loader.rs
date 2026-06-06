use std::collections::HashMap;
use sqlx::PgPool;
use plant_config::{
    PlantConfig, PlcConfig, DeviceConfig, InputVariable, DeviceTypeDefinition,
};

// ============================================================================
// Full plant load — used at startup
// ============================================================================

pub async fn load(
    pool: &PgPool,
) -> Result<(PlantConfig, Vec<DeviceTypeDefinition>), Box<dyn std::error::Error + Send + Sync>> {
    let device_types = load_device_types(pool).await?;
    load_impl(pool, &device_types).await
}

async fn load_impl(
    pool:         &PgPool,
    device_types: &[DeviceTypeDefinition],
) -> Result<(PlantConfig, Vec<DeviceTypeDefinition>), Box<dyn std::error::Error + Send + Sync>> {
    #[derive(sqlx::FromRow)]
    struct PlcRow {
        id:           uuid::Uuid,
        plc_id:       Option<String>,
        name:         String,
        kind:         String,
        deploy_node:  Option<String>,
        endpoint_uri: Option<String>,
        port:         Option<i32>,
        protocol:     String,
        endpoint:     String,
    }

    let plc_rows = sqlx::query_as::<_, PlcRow>(
        "SELECT id, plc_id, name, kind::TEXT as kind, deploy_node, endpoint_uri, port, protocol, endpoint
         FROM plcs ORDER BY name"
    ).fetch_all(pool).await?;

    #[derive(sqlx::FromRow)]
    struct InstRow {
        plc_id:       uuid::Uuid,
        device_id:    Option<String>,
        name:         String,
        dt_name:      String,
        param_values: serde_json::Value,
    }

    let inst_rows = sqlx::query_as::<_, InstRow>(
        "SELECT di.plc_id, di.device_id, di.name, dt.name AS dt_name, di.param_values
         FROM device_instances di
         JOIN device_types dt ON dt.id = di.device_type_id
         ORDER BY di.name"
    ).fetch_all(pool).await?;

    #[derive(sqlx::FromRow)]
    struct WireRow {
        dst_plc_id:     uuid::Uuid,
        dst_device_id:  Option<String>,
        dst_input_port: String,
        src_device_id:  Option<String>,
        src_field:      Option<String>,
    }

    let wire_rows = sqlx::query_as::<_, WireRow>(
        "SELECT di.plc_id AS dst_plc_id, di.device_id AS dst_device_id,
                w.dst_input_port, w.src_device_id, w.src_field
         FROM wires w
         JOIN device_instances di ON di.id = w.dst_instance_id
         WHERE w.src_device_id IS NOT NULL AND w.src_field IS NOT NULL"
    ).fetch_all(pool).await?;

    // (dst_plc_id, dst_device_id) → Vec<InputVariable>
    let mut wire_map: HashMap<(uuid::Uuid, String), Vec<InputVariable>> = HashMap::new();
    for w in wire_rows {
        if let (Some(dst_dev), Some(src_dev), Some(src_fld)) =
            (w.dst_device_id, w.src_device_id, w.src_field)
        {
            wire_map.entry((w.dst_plc_id, dst_dev)).or_default().push(InputVariable {
                name:             w.dst_input_port,
                source_device_id: src_dev,
                source_field:     src_fld,
            });
        }
    }

    let mut inst_by_plc: HashMap<uuid::Uuid, Vec<InstRow>> = HashMap::new();
    for inst in inst_rows {
        inst_by_plc.entry(inst.plc_id).or_default().push(inst);
    }

    let mut plcs = Vec::new();
    for p in plc_rows {
        let simulated = p.kind == "simulated";
        let plc_slug  = p.plc_id.unwrap_or_else(|| slugify(&p.name));

        let devices: Vec<DeviceConfig> = inst_by_plc.remove(&p.id).unwrap_or_default()
            .into_iter()
            .filter_map(|inst| {
                let device_id = inst.device_id?;
                let params    = json_to_params(&inst.param_values);
                let inputs    = wire_map.remove(&(p.id, device_id.clone())).unwrap_or_default();
                Some(DeviceConfig {
                    device_id,
                    name:            inst.name,
                    device_type:     inst.dt_name,
                    input_variables: inputs,
                    tick_ms:         None,
                    params,
                })
            })
            .collect();

        plcs.push(PlcConfig {
            plc_id:        plc_slug,
            name:          p.name,
            simulated,
            protocol:      p.protocol,
            uri:           p.endpoint_uri,
            port:          p.port.unwrap_or(4840) as u16,
            endpoint:      p.endpoint,
            deploy_target: p.deploy_node,
            devices,
        });
    }

    Ok((PlantConfig {
        plant_id:    "db".to_string(),
        name:        "Plant".to_string(),
        description: String::new(),
        plcs,
    }, device_types.to_vec()))
}

// ============================================================================
// Device types (shared helper)
// ============================================================================

async fn load_device_types(
    pool: &PgPool,
) -> Result<Vec<DeviceTypeDefinition>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(sqlx::FromRow)]
    struct Row { io_spec: serde_json::Value }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT io_spec FROM device_types ORDER BY name"
    ).fetch_all(pool).await?;

    let mut out = Vec::new();
    for row in rows {
        match serde_json::from_value::<DeviceTypeDefinition>(row.io_spec) {
            Ok(dt) => out.push(dt),
            Err(e) => tracing::warn!("Skipping unreadable device type in io_spec: {}", e),
        }
    }
    Ok(out)
}

// ============================================================================
// Wire table for the orchestration tick
// ============================================================================

#[derive(Clone)]
pub struct OrchestratorWire {
    pub src_device_id: String,
    pub src_field:     String,
    pub dst_plc_name:  String,
    /// Pre-computed: "ns=2;s={plc_name}.{device_id}.{input_port}"
    pub dst_node_id:   String,
}

pub async fn load_wires(
    pool: &PgPool,
) -> Result<Vec<OrchestratorWire>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        src_device_id:  Option<String>,
        src_field:      Option<String>,
        dst_plc_name:   String,
        dst_device_id:  Option<String>,
        dst_input_port: String,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT w.src_device_id, w.src_field,
                p.name AS dst_plc_name, di.device_id AS dst_device_id, w.dst_input_port
         FROM wires w
         JOIN device_instances di ON di.id = w.dst_instance_id
         JOIN plcs p               ON p.id = di.plc_id
         WHERE w.src_device_id IS NOT NULL
           AND w.src_field     IS NOT NULL
           AND di.device_id    IS NOT NULL"
    ).fetch_all(pool).await?;

    Ok(rows.into_iter().filter_map(|r| {
        Some(OrchestratorWire {
            src_device_id: r.src_device_id?,
            src_field:     r.src_field?,
            dst_node_id:   format!("ns=2;s={}.{}.{}", r.dst_plc_name, r.dst_device_id?, r.dst_input_port),
            dst_plc_name:  r.dst_plc_name,
        })
    }).collect())
}

// ============================================================================
// Single-PLC config (simulator self-config endpoint)
// ============================================================================

pub async fn load_plc_config(
    pool:     &PgPool,
    plc_uuid: uuid::Uuid,
) -> Result<Option<(PlcConfig, Vec<DeviceTypeDefinition>)>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(sqlx::FromRow)]
    struct PlcRow {
        plc_id:       Option<String>,
        name:         String,
        kind:         String,
        deploy_node:  Option<String>,
        endpoint_uri: Option<String>,
        port:         Option<i32>,
        protocol:     String,
        endpoint:     String,
    }

    let Some(p) = sqlx::query_as::<_, PlcRow>(
        "SELECT plc_id, name, kind::TEXT AS kind, deploy_node, endpoint_uri, port, protocol, endpoint
         FROM plcs WHERE id = $1"
    ).bind(plc_uuid).fetch_optional(pool).await? else {
        return Ok(None);
    };

    let plc_slug = p.plc_id.unwrap_or_else(|| slugify(&p.name));

    #[derive(sqlx::FromRow)]
    struct InstRow {
        device_id:    Option<String>,
        name:         String,
        dt_name:      String,
        param_values: serde_json::Value,
        io_spec:      serde_json::Value,
    }

    let inst_rows = sqlx::query_as::<_, InstRow>(
        "SELECT di.device_id, di.name, dt.name AS dt_name, di.param_values, dt.io_spec
         FROM device_instances di
         JOIN device_types dt ON dt.id = di.device_type_id
         WHERE di.plc_id = $1
         ORDER BY di.name"
    ).bind(plc_uuid).fetch_all(pool).await?;

    #[derive(sqlx::FromRow)]
    struct WireRow {
        dst_device_id:  Option<String>,
        dst_input_port: String,
        src_device_id:  Option<String>,
        src_field:      Option<String>,
    }

    let wire_rows = sqlx::query_as::<_, WireRow>(
        "SELECT di.device_id AS dst_device_id, w.dst_input_port, w.src_device_id, w.src_field
         FROM wires w
         JOIN device_instances di ON di.id = w.dst_instance_id
         WHERE di.plc_id = $1
           AND w.src_device_id IS NOT NULL AND w.src_field IS NOT NULL"
    ).bind(plc_uuid).fetch_all(pool).await?;

    let mut wire_map: HashMap<String, Vec<InputVariable>> = HashMap::new();
    for w in wire_rows {
        if let (Some(dst_dev), Some(src_dev), Some(src_fld)) =
            (w.dst_device_id, w.src_device_id, w.src_field)
        {
            wire_map.entry(dst_dev).or_default().push(InputVariable {
                name:             w.dst_input_port,
                source_device_id: src_dev,
                source_field:     src_fld,
            });
        }
    }

    let mut seen_types  = std::collections::HashSet::new();
    let mut device_types = Vec::new();
    let mut devices      = Vec::new();

    for inst in inst_rows {
        let Some(device_id) = inst.device_id else { continue };
        let inputs = wire_map.remove(&device_id).unwrap_or_default();

        devices.push(DeviceConfig {
            device_id:       device_id.clone(),
            name:            inst.name,
            device_type:     inst.dt_name.clone(),
            input_variables: inputs,
            tick_ms:         None,
            params:          json_to_params(&inst.param_values),
        });

        if seen_types.insert(inst.dt_name.clone()) {
            match serde_json::from_value::<DeviceTypeDefinition>(inst.io_spec) {
                Ok(dt) => device_types.push(dt),
                Err(e) => tracing::warn!("Unreadable io_spec for type '{}': {}", inst.dt_name, e),
            }
        }
    }

    let plc = PlcConfig {
        plc_id:        plc_slug,
        name:          p.name,
        simulated:     p.kind == "simulated",
        protocol:      p.protocol,
        uri:           p.endpoint_uri,
        port:          p.port.unwrap_or(4840) as u16,
        endpoint:      p.endpoint,
        deploy_target: p.deploy_node,
        devices,
    };

    Ok(Some((plc, device_types)))
}

// ============================================================================
// Helpers
// ============================================================================

pub fn json_to_params(v: &serde_json::Value) -> HashMap<String, f64> {
    v.as_object()
        .map(|obj| obj.iter()
            .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
            .collect())
        .unwrap_or_default()
}

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
