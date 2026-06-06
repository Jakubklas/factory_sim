use std::sync::Arc;
use tokio::sync::RwLock;
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
mod orchestration;
mod k8s;

use config_handle::load_all;
use comms::DiscoveredState;

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

    let asset_root  = std::env::var("ASSET_DIR").unwrap_or_else(|_| "/data/assets".into());
    let asset_store = Arc::new(assets::LocalStore::new(asset_root));
    let discovered: Arc<RwLock<DiscoveredState>> = Arc::new(RwLock::new(std::collections::HashMap::new()));

    let addr = format!("{}:{}", app.ws_host, app.ws_port);
    tracing::info!(
        "\n\n  ══════════════════════════════════\n  water-plant-twin  starting\n  ══════════════════════════════════\n\n  \
         API  {}\n\n    \
         ws://{}/ws\n    \
         http://{}/api/plant\n    \
         http://{}/api/device-types\n    \
         http://{}/api/plcs\n    \
         http://{}/api/setpoint  (POST)\n    \
         http://{}/api/log-level?set=info\n",
        addr, addr, addr, addr, addr, addr, addr
    );

    let (ingested, write_handles) =
        plant::start(plant.clone(), app.tick_ms, Arc::clone(&discovered)).await?;

    let write_handles_arc = Arc::new(write_handles);
    orchestration::start(
        Arc::clone(&plant),
        app.tick_ms,
        Arc::clone(&ingested),
        Arc::clone(&write_handles_arc),
    );

    let ingested_ws   = ingested.clone();
    let discovered_ws = Arc::clone(&discovered);
    let ws_host       = app.ws_host.clone();
    let plant_ws      = Arc::clone(&plant);
    let db_pool_ws    = db_pool.clone();

    tokio::spawn(async move {
        if let Err(e) = api::ws_bridge::start(
            ingested_ws, plant_ws, app.tick_ms, &ws_host, app.ws_port, log_handle,
            db_pool_ws, asset_store, discovered_ws, Arc::clone(&write_handles_arc),
        ).await {
            tracing::error!("WS bridge error: {}", e);
        }
    });

    // K8s reconciler — creates/deletes sim PLC pods to match desired state.
    // Reads SIMULATOR_IMAGE, BACKEND_SVC_URL, K8S_NAMESPACE from env.
    // Silently skips if kubectl is not in PATH (dev / non-cluster mode).
    {
        let sim_image   = std::env::var("SIMULATOR_IMAGE").unwrap_or_else(|_| "factory-sim/simulator:latest".into());
        let backend_url = std::env::var("BACKEND_SVC_URL").unwrap_or_else(|_| format!("http://backend:{}", app.ws_port));
        let namespace   = std::env::var("K8S_NAMESPACE").unwrap_or_else(|_| "default".into());
        k8s::start(Arc::clone(&plant), sim_image, backend_url, namespace, db_pool);
    }

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
