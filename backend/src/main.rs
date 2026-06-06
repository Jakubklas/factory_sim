use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt, reload};

struct SecondsTimer;
impl tracing_subscriber::fmt::time::FormatTime for SecondsTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"))
    }
}

mod config_handle;
mod comms;
mod plant;
mod api;
mod db;
mod assets;

use config_handle::load_all;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::from_default_env()
        .add_directive("info".parse()?)
        .add_directive("opcua=warn".parse()?);

    let (filter_layer, log_handle) = reload::Layer::new(filter);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer().with_timer(SecondsTimer))
        .init();

    let (app, plant) = load_all()?;

    // Database — optional: if DATABASE_URL is unset the DB features are skipped.
    let db_pool = match std::env::var("DATABASE_URL") {
        Ok(url) => {
            tracing::info!("Connecting to database…");
            let pool = db::connect(&url).await?;
            db::seed::seed_from_configs(&pool).await?;
            Some(pool)
        }
        Err(_) => {
            tracing::warn!("DATABASE_URL not set — running without persistence");
            None
        }
    };

    let asset_root = std::env::var("ASSET_DIR").unwrap_or_else(|_| "/data/assets".into());
    let asset_store = std::sync::Arc::new(assets::LocalStore::new(asset_root));

    let addr = format!("{}:{}", app.ws_host, app.ws_port);
    tracing::info!(
        "\n\n  ══════════════════════════════════\n  water-plant-twin  starting\n  ══════════════════════════════════\n\n  \
         API  {}\n\n    \
         ws://{}/ws\n    \
         http://{}/api/plant\n    \
         http://{}/api/device-types\n    \
         http://{}/api/plcs\n    \
         http://{}/api/log-level?set=info\n",
        addr, addr, addr, addr, addr, addr
    );

    let ingested    = plant::start(plant.clone(), app.tick_ms).await?;
    let ingested_ws = ingested.clone();
    let ws_host     = app.ws_host.clone();

    tokio::spawn(async move {
        if let Err(e) = api::ws_bridge::start(
            ingested_ws, plant, app.tick_ms, &ws_host, app.ws_port, log_handle,
            db_pool, asset_store,
        ).await {
            tracing::error!("WS bridge error: {}", e);
        }
    });

    tracing::info!("\n  ══ Running — press Ctrl-C or send SIGTERM to stop ══\n");
    shutdown_signal().await;
    tracing::info!("\n  Shutdown signal received — exiting\n");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.ok(); };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sig) = signal(SignalKind::terminate()) { sig.recv().await; }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
