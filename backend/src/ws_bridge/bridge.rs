use axum::{
    extract::{ws::{WebSocket, WebSocketUpgrade}, State},
    response::Response,
    routing::get,
    Router,
};
use axum::extract::ws::Message;
use tower_http::cors::CorsLayer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use crate::simulator::plant::PlantState;

#[derive(Clone)]
pub struct AppState {
    pub tx: broadcast::Sender<PlantState>,
}

pub async fn start_ws_server(tx: broadcast::Sender<PlantState>) -> Result<(), Box<dyn std::error::Error>> {
    let app_state = AppState { tx };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("WebSocket server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    tracing::info!("New WebSocket connection");

    let mut rx = state.tx.subscribe();

    loop {
        tokio::select! {
            // Receive plant state updates and send to client
            Ok(plant_state) = rx.recv() => {
                let msg = serde_json::json!({
                    "type": "snapshot",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "devices": plant_state.devices
                });

                if let Ok(json) = serde_json::to_string(&msg) {
                    if socket.send(Message::Text(json)).await.is_err() {
                        tracing::info!("Client disconnected");
                        break;
                    }
                }
            }
            // Handle incoming messages from client
            result = socket.recv() => {
                match result {
                    Some(Ok(_msg)) => {
                        // Could handle commands here
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        tracing::info!("WebSocket connection closed");
                        break;
                    }
                }
            }
        }
    }
}
