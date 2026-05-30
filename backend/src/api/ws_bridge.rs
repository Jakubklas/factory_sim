use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use axum::{
    Json,
    Router,
    extract::{State, Query, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use tower_http::cors::CorsLayer;
use tracing_subscriber::{EnvFilter, reload};
use plant_config::{ResolvedPlant, PlantConfig};

use crate::comms::IngestedState;

type LogHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

#[derive(Clone)]
struct AppState {
    ingested:   Arc<RwLock<IngestedState>>,
    plant:      Arc<ResolvedPlant>,
    tick_ms:    u64,
    log_handle: LogHandle,
}

pub async fn start(
    ingested:   Arc<RwLock<IngestedState>>,
    plant:      Arc<ResolvedPlant>,
    tick_ms:    u64,
    host:       &str,
    port:       u16,
    log_handle: LogHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState { ingested, plant, tick_ms, log_handle };

    let app = Router::new()
        .route("/ws",            get(ws_handler))
        .route("/api/plant",     get(plant_handler))
        .route("/api/log-level", get(log_level_handler))
        .route("/health",        get(health_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr     = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    axum::serve(listener, app).await?;
    Ok(())
}

// ============================================================================
// Handlers
// ============================================================================

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        stream_state(socket, state.ingested, state.tick_ms).await
    })
}

async fn plant_handler(
    State(state): State<AppState>,
) -> Json<PlantConfig> {
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
        Err(e) => {
            (StatusCode::BAD_REQUEST, format!("invalid filter '{}': {}", filter_str, e))
        }
    }
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, [("content-type", "application/json")], r#"{"status":"ok"}"#)
}

async fn stream_state(mut socket: WebSocket, ingested: Arc<RwLock<IngestedState>>, tick_ms: u64) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
    let mut log_counter = 0;
    let log_interval = 10000 / tick_ms; // Log every 10 seconds
    
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
            tracing::info!("[WebSocket] Telemetry update sent to client - {} devices", snapshot.len());
            for (device_id, fields) in snapshot.iter().take(3) {
                tracing::info!("[WebSocket]   {}: {:?}", device_id, fields);
            }
        }
    }
}
