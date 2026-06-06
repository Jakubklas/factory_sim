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

use config_handle::AppConfig;
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

    let app = AppConfig::from_env()?;

    // ── Database ───────────────────────────────────────────────────────────────
    let db_pool = match std::env::var("DATABASE_URL") {
        Ok(url) => {
            tracing::info!("Connecting to database…");
            Some(db::connect(&url).await?)
        }
        Err(_) => {
            tracing::warn!("DATABASE_URL not set — running without persistence");
            None
        }
    };

    // ── Seed (only if SEED_DIR set AND DB is empty) ────────────────────────────
    if let Some(pool) = &db_pool {
        db::seed::seed_if_configured(pool).await?;
    }

    // ── Load plant topology ────────────────────────────────────────────────────
    // Primary path: load from DB.
    // Fallback: load from PLANT_CONFIG directory (local dev without DB).
    let plant = if let Some(pool) = &db_pool {
        let (plant_config, device_types) = db::plant_loader::load(pool).await
            .map_err(|e| e.to_string())?;
        Arc::new(plant_config::ResolvedPlant::build(plant_config, device_types)?)
    } else {
        // No DB — try PLANT_CONFIG for backward compat.
        let config_dir = std::env::var("PLANT_CONFIG").unwrap_or_else(|_| "/config".into());
        tracing::warn!("No DB — falling back to PLANT_CONFIG={}", config_dir);
        let dir = std::path::Path::new(&config_dir);
        let pc  = plant_config::loader::load_plant_config(dir.join("plant.json").to_str().unwrap())?;
        let dts = plant_config::loader::load_device_types(dir.join("device_types.json").to_str().unwrap())?;
        Arc::new(plant_config::ResolvedPlant::build(pc, dts)?)
    };

    let asset_root  = std::env::var("ASSET_DIR").unwrap_or_else(|_| "/data/assets".into());
    let asset_store = Arc::new(assets::LocalStore::new(asset_root));
    let discovered: Arc<RwLock<DiscoveredState>> = Arc::new(RwLock::new(std::collections::HashMap::new()));

    // ── PLC connectors ─────────────────────────────────────────────────────────
    let (ingested, write_handles) =
        plant::start(Arc::clone(&plant), app.tick_ms, Arc::clone(&discovered)).await?;

    // ── Orchestration tick ─────────────────────────────────────────────────────
    let wires = if let Some(pool) = &db_pool {
        orchestration::load(pool).await.map_err(|e| e.to_string())?
    } else {
        // Build wire table from in-memory plant config when no DB is available.
        let wires = build_wire_table_from_plant(&plant);
        Arc::new(RwLock::new(wires))
    };

    orchestration::start(
        Arc::clone(&wires),
        app.tick_ms,
        Arc::clone(&ingested),
        Arc::clone(&write_handles),
    );

    // ── Postgres LISTEN loop ───────────────────────────────────────────────────
    // Reacts to INSERT/UPDATE/DELETE on the `plcs` table within one reconcile
    // interval — starts new connectors and refreshes the wire table without restart.
    if let Some(pool) = db_pool.clone() {
        let pool2       = pool.clone();
        let ingested2   = Arc::clone(&ingested);
        let disc2       = Arc::clone(&discovered);
        let handles2    = Arc::clone(&write_handles);
        let wires2      = Arc::clone(&wires);
        let tick_ms     = app.tick_ms;

        tokio::spawn(async move {
            match run_listen_loop(pool2, ingested2, disc2, handles2, wires2, tick_ms).await {
                Ok(()) => {},
                Err(e) => tracing::error!("[listen] Fatal error in LISTEN loop: {}", e),
            }
        });
    }

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

    // ── K8s reconciler ─────────────────────────────────────────────────────────
    if let Some(pool) = db_pool.clone() {
        let sim_image   = std::env::var("SIMULATOR_IMAGE").unwrap_or_else(|_| "factory-sim/simulator:latest".into());
        let backend_url = std::env::var("BACKEND_SVC_URL").unwrap_or_else(|_| format!("http://backend:{}", app.ws_port));
        let namespace   = std::env::var("K8S_NAMESPACE").unwrap_or_else(|_| "default".into());
        k8s::start(pool, sim_image, backend_url, namespace);
    }

    // ── API server ─────────────────────────────────────────────────────────────
    let ingested_ws   = ingested.clone();
    let discovered_ws = Arc::clone(&discovered);
    let ws_host       = app.ws_host.clone();
    let plant_ws      = Arc::clone(&plant);
    let db_pool_ws    = db_pool.clone();

    tokio::spawn(async move {
        if let Err(e) = api::ws_bridge::start(
            ingested_ws, plant_ws, app.tick_ms, &ws_host, app.ws_port, log_handle,
            db_pool_ws, asset_store, discovered_ws, Arc::clone(&write_handles),
        ).await {
            tracing::error!("WS bridge error: {}", e);
        }
    });

    tracing::info!("\n  ══ Running — press Ctrl-C or send SIGTERM to stop ══\n");
    shutdown_signal().await;
    tracing::info!("\n  Shutdown signal received — exiting\n");
    Ok(())
}

// ── Postgres LISTEN loop ───────────────────────────────────────────────────────

async fn run_listen_loop(
    pool:          sqlx::PgPool,
    ingested:      Arc<RwLock<comms::IngestedState>>,
    discovered:    Arc<RwLock<DiscoveredState>>,
    write_handles: plant::WriteHandles,
    wires:         Arc<RwLock<Vec<db::plant_loader::OrchestratorWire>>>,
    tick_ms:       u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool).await?;
    listener.listen("plant_changed").await?;
    tracing::info!("[listen] Listening on plant_changed");

    loop {
        let _notification = listener.recv().await?;
        tracing::info!("[listen] plant_changed — refreshing connectors and wires");

        // Reload wire table from DB.
        if let Err(e) = orchestration::refresh_wires(&pool, &wires).await {
            tracing::warn!("[listen] Failed to refresh wire table: {}", e);
        }

        // Start any new PLC connectors not yet in the handle map.
        match db::plant_loader::load(&pool).await {
            Ok((plant_config, _)) => {
                for plc in &plant_config.plcs {
                    plant::add_plc_connector(
                        plc, tick_ms,
                        Arc::clone(&ingested),
                        Arc::clone(&discovered),
                        Arc::clone(&write_handles),
                    ).await;
                }
            }
            Err(e) => tracing::warn!("[listen] Failed to reload plant from DB: {}", e),
        }
    }
}

// ── Fallback wire table (no-DB mode) ─────────────────────────────────────────

fn build_wire_table_from_plant(
    plant: &plant_config::ResolvedPlant,
) -> Vec<db::plant_loader::OrchestratorWire> {
    let device_to_plc: std::collections::HashMap<&str, &str> = plant.config.plcs.iter()
        .flat_map(|plc| plc.devices.iter().map(move |d| (d.device_id.as_str(), plc.name.as_str())))
        .collect();

    let mut wires = Vec::new();
    for device in &plant.devices {
        let dst_id = &device.config.device_id;
        let Some(&dst_plc) = device_to_plc.get(dst_id.as_str()) else { continue };
        for input in &device.config.input_variables {
            wires.push(db::plant_loader::OrchestratorWire {
                src_device_id: input.source_device_id.clone(),
                src_field:     input.source_field.clone(),
                dst_plc_name:  dst_plc.to_string(),
                dst_node_id:   format!("ns=2;s={}.{}.{}", dst_plc, dst_id, input.name),
            });
        }
    }
    wires
}

// ── Shutdown ──────────────────────────────────────────────────────────────────

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
