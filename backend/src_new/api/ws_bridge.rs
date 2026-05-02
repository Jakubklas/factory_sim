use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    response::Response,
    routing::get,
};
use tower_http::cors::CorsLayer;

use crate::comms::IngestedState;

/// Start the axum server and block until it exits.
/// Spawn this in a tokio task from main so it runs alongside the plant.
pub async fn start(
    ingested: Arc<RwLock<IngestedState>>,
    tick_ms:  u64,
    host:     &str,
    port:     u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state((ingested, tick_ms));

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("WS bridge listening on ws://{}/ws", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

// ============================================================================
// Handlers
// ============================================================================

async fn ws_handler(
    ws: WebSocketUpgrade,
    State((ingested, tick_ms)): State<(Arc<RwLock<IngestedState>>, u64)>, // axum clones & injects whatever was passed to .with_state()
) -> Response {
    ws.on_upgrade(move |socket| async move {
        stream_state(socket, ingested, tick_ms).await
    })
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
