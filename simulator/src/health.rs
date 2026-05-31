use axum::{Router, routing::get};

pub fn start(port: u16) {
    let app = Router::new().route("/health", get(handler));
    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await
            .expect("health server bind failed");
        tracing::info!("Health endpoint: http://{}/health", addr);
        axum::serve(listener, app).await
            .expect("health server error");
    });
}

async fn handler() -> &'static str {
    r#"{"status":"ok"}"#
}
