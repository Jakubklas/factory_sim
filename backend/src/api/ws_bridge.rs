use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{
    Json,
    Router,
    extract::{State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    response::Response,
    routing::get,
};
use tower_http::cors::CorsLayer;

use crate::comms::{IngestedState, release_ports};
use crate::config_handle::PlantConfigHandle;
use crate::config_handle::schema::PlantConfig;

#[derive(Clone)]
struct AppState {
    ingested: Arc<RwLock<IngestedState>>,
    handle:   Arc<RwLock<PlantConfigHandle>>,
    tick_ms:  u64,
}

/// Start the axum server and block until it exits.
/// Spawn this in a tokio task from main so it runs alongside the plant.
pub async fn start(
    ingested: Arc<RwLock<IngestedState>>,
    handle:   Arc<RwLock<PlantConfigHandle>>,
    tick_ms:  u64,
    host:     &str,
    port:     u16,
) -> Result<(), Box<dyn std::error::Error>> {
    release_ports(&[port]);

    let state = AppState { ingested, handle, tick_ms };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/plant", get(plant_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("WS bridge listening on ws://{}/ws", addr);
    tracing::info!("Plant API available at http://{}/api/plant", addr);

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
    let config = state.handle.read().await.plant_config().clone();
    Json(config)
}

/// Push the full IngestedState snapshot to the client on every tick.
/// Breaks cleanly on send error (client disconnected).
async fn stream_state(mut socket: WebSocket, ingested: Arc<RwLock<IngestedState>>, tick_ms: u64) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
    loop {
        interval.tick().await;

        let snapshot = ingested.read().await.clone();
        let Ok(json) = serde_json::to_string(&snapshot) else { continue };

        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
    }
}
