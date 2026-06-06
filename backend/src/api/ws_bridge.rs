use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use axum::{
    Json,
    Router,
    extract::{Path, State, Query, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use tower_http::cors::CorsLayer;
use tracing_subscriber::{EnvFilter, reload};
use uuid::Uuid;
use plant_config::{ResolvedPlant, PlantConfig, DeviceTypeDefinition};
use sqlx::PgPool;

use crate::assets::LocalStore;
use crate::comms::{BrowsedNode, DiscoveredState, IngestedState, WriteCmd, WriteHandle};
use crate::db::{models, queries};

type LogHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

#[derive(Clone)]
struct AppState {
    ingested:      Arc<RwLock<IngestedState>>,
    plant:         Arc<ResolvedPlant>,
    tick_ms:       u64,
    log_handle:    LogHandle,
    db:            Option<PgPool>,
    #[allow(dead_code)]
    assets:        Arc<LocalStore>,
    discovered:    Arc<RwLock<DiscoveredState>>,
    write_handles: Arc<std::collections::HashMap<String, WriteHandle>>,
}

pub async fn start(
    ingested:      Arc<RwLock<IngestedState>>,
    plant:         Arc<ResolvedPlant>,
    tick_ms:       u64,
    host:          &str,
    port:          u16,
    log_handle:    LogHandle,
    db:            Option<PgPool>,
    assets:        Arc<LocalStore>,
    discovered:    Arc<RwLock<DiscoveredState>>,
    write_handles: Arc<std::collections::HashMap<String, WriteHandle>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState {
        ingested, plant, tick_ms, log_handle, db, assets, discovered,
        write_handles,
    };

    let app = Router::new()
        // Real-time
        .route("/ws",            get(ws_handler))
        .route("/api/plant",     get(plant_handler))
        .route("/api/log-level", get(log_level_handler))
        .route("/health",        get(health_handler))
        // Deploy nodes
        .route("/api/deploy-nodes", get(list_deploy_nodes))
        // Device types
        .route("/api/device-types",     get(list_device_types).post(create_device_type))
        .route("/api/device-types/:id", delete(delete_device_type))
        // PLCs
        .route("/api/plcs",     get(list_plcs).post(create_plc))
        .route("/api/plcs/:id", delete(delete_plc))
        // Device instances (per PLC)
        .route("/api/plcs/:plc_id/instances",     get(list_instances).post(create_instance))
        .route("/api/plcs/:plc_id/instances/:id", delete(delete_instance))
        // Discovered nodes (per PLC — populated by browse after each connect)
        .route("/api/plcs/:plc_id/discovered", get(list_discovered))
        // Simulator self-build config (fetched by sim pods on startup)
        .route("/api/plcs/:plc_id/config", get(sim_config))
        // Setpoint write
        .route("/api/setpoint", axum::routing::post(write_setpoint))
        // Wires
        .route("/api/wires",     get(list_wires).post(create_wire))
        .route("/api/wires/:id", delete(delete_wire))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr     = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

fn db_required(db: &Option<PgPool>) -> Result<&PgPool, (StatusCode, String)> {
    db.as_ref().ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, "DATABASE_URL not configured".into()))
}

fn db_err(e: sqlx::Error) -> (StatusCode, String) {
    tracing::error!("DB error: {}", e);
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ============================================================================
// Deploy nodes
// ============================================================================

async fn list_deploy_nodes(State(s): State<AppState>) -> Result<Json<Vec<models::DeployNode>>, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    queries::list_deploy_nodes(pool).await.map(Json).map_err(db_err)
}

// ============================================================================
// Device types
// ============================================================================

async fn list_device_types(State(s): State<AppState>) -> Result<Json<Vec<models::DeviceType>>, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    queries::list_device_types(pool).await.map(Json).map_err(db_err)
}

async fn create_device_type(
    State(s): State<AppState>,
    Json(req): Json<models::CreateDeviceType>,
) -> Result<(StatusCode, Json<models::DeviceType>), (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let dt = queries::create_device_type(pool, &req).await.map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(dt)))
}

async fn delete_device_type(
    Path(id): Path<Uuid>,
    State(s): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let n = queries::delete_device_type(pool, id).await.map_err(db_err)?;
    if n == 0 { Err((StatusCode::NOT_FOUND, "not found".into())) } else { Ok(StatusCode::NO_CONTENT) }
}

// ============================================================================
// PLCs
// ============================================================================

async fn list_plcs(State(s): State<AppState>) -> Result<Json<Vec<models::Plc>>, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    queries::list_plcs(pool).await.map(Json).map_err(db_err)
}

async fn create_plc(
    State(s): State<AppState>,
    Json(req): Json<models::CreatePlc>,
) -> Result<(StatusCode, Json<models::Plc>), (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let plc = queries::create_plc(pool, &req).await.map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(plc)))
}

async fn delete_plc(
    Path(id): Path<Uuid>,
    State(s): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let n = queries::delete_plc(pool, id).await.map_err(db_err)?;
    if n == 0 { Err((StatusCode::NOT_FOUND, "not found".into())) } else { Ok(StatusCode::NO_CONTENT) }
}

// ============================================================================
// Device instances
// ============================================================================

async fn list_instances(
    Path(plc_id): Path<Uuid>,
    State(s): State<AppState>,
) -> Result<Json<Vec<models::DeviceInstance>>, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    queries::list_device_instances_for_plc(pool, plc_id).await.map(Json).map_err(db_err)
}

async fn create_instance(
    Path(_plc_id): Path<Uuid>,
    State(s): State<AppState>,
    Json(req): Json<models::CreateDeviceInstance>,
) -> Result<(StatusCode, Json<models::DeviceInstance>), (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let inst = queries::create_device_instance(pool, &req).await.map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(inst)))
}

async fn delete_instance(
    Path((_plc_id, id)): Path<(Uuid, Uuid)>,
    State(s): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let n = queries::delete_device_instance(pool, id).await.map_err(db_err)?;
    if n == 0 { Err((StatusCode::NOT_FOUND, "not found".into())) } else { Ok(StatusCode::NO_CONTENT) }
}

// ============================================================================
// Discovered nodes — served from in-memory browse cache.
// UUID→name lookup via DB when available; falls back to matching plant config name.
// ============================================================================

async fn list_discovered(
    Path(plc_id): Path<Uuid>,
    State(s): State<AppState>,
) -> Result<Json<Vec<BrowsedNode>>, (StatusCode, String)> {
    // Resolve PLC name from UUID.
    let plc_name: Option<String> = if let Some(pool) = &s.db {
        sqlx::query_as::<_, (String,)>("SELECT name FROM plcs WHERE id = $1")
            .bind(plc_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .map(|(n,)| n)
    } else {
        // No DB — try to find in plant config by matching the UUID string in the plc_id field.
        // (plant_config plc_id is like "plc-001", not a UUID, so this path returns None unless
        // the user supplied a name-based UUID; Phase 5 will make this authoritative.)
        None
    };

    let name = plc_name.ok_or_else(|| (StatusCode::NOT_FOUND, format!("PLC {} not found", plc_id)))?;
    let map  = s.discovered.read().await;
    let nodes = map.get(&name).cloned().unwrap_or_default();
    Ok(Json(nodes))
}

// ============================================================================
// Simulator self-build config — GET /api/plcs/:plc_id/config
//
// A simulator pod fetches this on startup instead of reading plant.json from a
// ConfigMap. :plc_id is either the plant.json plc_id string ("plc-001") or,
// when using DB-backed provisioning, a UUID that maps to a name.
//
// Returns: { "plc": <PlcConfig>, "device_types": [<DeviceTypeDefinition>] }
// The simulator deserializes these and calls ResolvedPlant::build().
// ============================================================================

#[derive(serde::Serialize)]
struct SimConfigResponse {
    plc:          plant_config::PlcConfig,
    device_types: Vec<DeviceTypeDefinition>,
}

async fn sim_config(
    Path(plc_id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<SimConfigResponse>, (StatusCode, String)> {
    // Try to find the PLC in the loaded plant by plc_id string first.
    // If a UUID was given, also try matching against DB name.
    let plc = s.plant.config.plcs.iter()
        .find(|p| p.plc_id == plc_id)
        .cloned();

    // If not found by plc_id, try DB name lookup (UUID → name → plc_id match).
    let plc = if plc.is_none() {
        if let Ok(uuid) = plc_id.parse::<uuid::Uuid>() {
            if let Some(pool) = &s.db {
                let row = sqlx::query_as::<_, (String,)>("SELECT name FROM plcs WHERE id = $1")
                    .bind(uuid)
                    .fetch_optional(pool)
                    .await
                    .map_err(db_err)?;
                row.and_then(|(name,)| {
                    s.plant.config.plcs.iter().find(|p| p.name == name).cloned()
                })
            } else { None }
        } else { None }
    } else { plc };

    let plc = plc.ok_or_else(|| (StatusCode::NOT_FOUND, format!("PLC '{}' not found", plc_id)))?;

    if !plc.simulated {
        return Err((StatusCode::BAD_REQUEST, "config endpoint is only for simulated PLCs".into()));
    }

    // Collect unique device types referenced by this PLC's devices.
    let plc_device_types: std::collections::HashSet<String> = plc.devices.iter()
        .map(|d| d.device_type.clone())
        .collect();
    let mut seen = std::collections::HashSet::new();
    let device_types: Vec<DeviceTypeDefinition> = s.plant.devices.iter()
        .filter(|d| plc_device_types.contains(&d.type_def.device_type))
        .filter(|d| seen.insert(d.type_def.device_type.clone()))
        .map(|d| d.type_def.clone())
        .collect();

    Ok(Json(SimConfigResponse { plc, device_types }))
}

// ============================================================================
// Setpoint write — POST /api/setpoint
// Body: { "plc_name": "...", "node_id": "ns=2;s=...", "value": 42.0 }
// ============================================================================

#[derive(serde::Deserialize)]
struct SetpointRequest {
    plc_name: String,
    node_id:  String,
    value:    serde_json::Value,
}

async fn write_setpoint(
    State(s): State<AppState>,
    Json(req): Json<SetpointRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    use plant_config::DataType;

    let value = match &req.value {
        serde_json::Value::Number(n) => DataType::Float(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::Bool(b)   => DataType::Boolean(*b),
        serde_json::Value::String(s) => DataType::Str(s.clone()),
        other => return Err((StatusCode::BAD_REQUEST, format!("unsupported value type: {}", other))),
    };

    let handle = s.write_handles.get(&req.plc_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, format!("no connector for PLC '{}'", req.plc_name))
    })?;

    handle.send(WriteCmd { node_id: req.node_id, value });
    Ok(StatusCode::ACCEPTED)
}

// ============================================================================
// Wires
// ============================================================================

async fn list_wires(State(s): State<AppState>) -> Result<Json<Vec<models::Wire>>, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    queries::list_wires(pool).await.map(Json).map_err(db_err)
}

async fn create_wire(
    State(s): State<AppState>,
    Json(req): Json<models::CreateWire>,
) -> Result<(StatusCode, Json<models::Wire>), (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let wire = queries::create_wire(pool, &req).await.map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(wire)))
}

async fn delete_wire(
    Path(id): Path<Uuid>,
    State(s): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = db_required(&s.db)?;
    let n = queries::delete_wire(pool, id).await.map_err(db_err)?;
    if n == 0 { Err((StatusCode::NOT_FOUND, "not found".into())) } else { Ok(StatusCode::NO_CONTENT) }
}

// ============================================================================
// Legacy / real-time handlers
// ============================================================================

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| async move {
        stream_state(socket, state.ingested, state.tick_ms).await
    })
}

async fn plant_handler(State(state): State<AppState>) -> Json<PlantConfig> {
    Json(state.plant.config.clone())
}

async fn log_level_handler(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(filter_str) = params.get("set") else {
        return (StatusCode::OK, [
            "Usage: GET /api/log-level?set=<RUST_LOG filter>",
            "Examples:",
            "  ?set=debug",
            "  ?set=backend::comms=trace",
            "  ?set=info                            (reset to default)",
        ].join("\n"));
    };

    match EnvFilter::try_new(filter_str) {
        Ok(new_filter) => {
            state.log_handle.reload(new_filter).ok();
            tracing::info!("Log filter changed to: {}", filter_str);
            (StatusCode::OK, format!("filter set to: {}", filter_str))
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("invalid filter '{}': {}", filter_str, e))
    }
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, [("content-type", "application/json")], r#"{"status":"ok"}"#)
}

async fn stream_state(mut socket: WebSocket, ingested: Arc<RwLock<IngestedState>>, tick_ms: u64) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
    let mut log_counter = 0u64;
    let log_interval = 10000 / tick_ms;

    loop {
        interval.tick().await;
        log_counter += 1;

        let snapshot = ingested.read().await.clone();
        let Ok(json) = serde_json::to_string(&snapshot) else { continue };

        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }

        if log_counter >= log_interval {
            log_counter = 0;
            tracing::info!("[WebSocket] Telemetry update — {} devices", snapshot.len());
        }
    }
}
