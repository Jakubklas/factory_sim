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

    let app = Arc::new(AppConfig::from_env()?);

    // ── Database ───────────────────────────────────────────────────────────────
    let db_pool = match &app.database_url {
        Some(url) => {
            tracing::info!("Connecting to database…");
            Some(db::connect(url).await?)
        }
        None => {
            tracing::warn!("DATABASE_URL not set — running without persistence");
            None
        }
    };

    // ── Seed (only if SEED_DIR set AND DB is empty) ────────────────────────────
    if let Some(pool) = &db_pool {
        db::seed::seed_if_configured(pool, app.seed_dir.as_deref()).await?;
    }

    // ── Load plant topology ────────────────────────────────────────────────────
    // Primary path: reconstruct PlantConfig from DB.
    // Fallback: read plant.json from PLANT_CONFIG directory (local dev without DB).
    let plant: Arc<plant_config::PlantConfig> = if let Some(pool) = &db_pool {
        let (plant_config, _device_types) = db::plant_loader::load(pool).await
            .map_err(|e| e.to_string())?;
        Arc::new(plant_config)
    } else {
        tracing::warn!("No DB — falling back to PLANT_CONFIG={}", app.plant_config_dir);
        let dir = std::path::Path::new(&app.plant_config_dir);
        let pc  = plant_config::loader::load_plant_config(dir.join("plant.json").to_str().unwrap())?;
        Arc::new(pc)
    };

    let asset_store = Arc::new(assets::LocalStore::new(app.asset_dir.clone()));
    let discovered: Arc<RwLock<DiscoveredState>> = Arc::new(RwLock::new(std::collections::HashMap::new()));

    // ── PLC connectors ─────────────────────────────────────────────────────────
    let (ingested, write_handles) = plant::start(
        Arc::clone(&plant), app.tick_ms, Arc::clone(&discovered),
        app.pki_dir.clone(), app.opcua_uri_override.clone(),
    ).await?;

    // ── Orchestration tick ─────────────────────────────────────────────────────
    let wires = if let Some(pool) = &db_pool {
        orchestration::load(pool).await.map_err(|e| e.to_string())?
    } else {
        let wires = build_wire_table_from_plant(&plant);
        Arc::new(RwLock::new(wires))
    };

    orchestration::start(
        Arc::clone(&wires),
        app.tick_ms,
        Arc::clone(&ingested),
        Arc::clone(&write_handles),
    );

    // Kick handle: lets the LISTEN loop trigger an immediate k8s reconcile on a plant
    // change instead of waiting up to RECONCILE_INTERVAL_SECS. The periodic reconcile
    // still runs as the drift-correcting safety net.
    let reconcile_kick = Arc::new(tokio::sync::Notify::new());

    // ── Postgres LISTEN loop ───────────────────────────────────────────────────
    if let Some(pool) = db_pool.clone() {
        let pool2       = pool.clone();
        let ingested2   = Arc::clone(&ingested);
        let disc2       = Arc::clone(&discovered);
        let handles2    = Arc::clone(&write_handles);
        let wires2      = Arc::clone(&wires);
        let app2        = Arc::clone(&app);
        let kick2       = Arc::clone(&reconcile_kick);

        tokio::spawn(async move {
            match run_listen_loop(pool2, ingested2, disc2, handles2, wires2, app2, kick2).await {
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
        k8s::start(
            pool,
            app.simulator_image.clone(),
            app.backend_svc_url.clone(),
            app.k8s_namespace.clone(),
            Arc::clone(&reconcile_kick),
        );
    }

    // ── API server ─────────────────────────────────────────────────────────────
    let ingested_ws   = ingested.clone();
    let discovered_ws = Arc::clone(&discovered);
    let ws_host       = app.ws_host.clone();
    let plant_ws      = Arc::clone(&plant);
    let db_pool_ws    = db_pool.clone();
    let app_ws        = Arc::clone(&app);

    tokio::spawn(async move {
        if let Err(e) = api::ws_bridge::start(
            ingested_ws, plant_ws, app_ws.tick_ms, &ws_host, app_ws.ws_port, log_handle,
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
    app:           Arc<AppConfig>,
    reconcile_kick: Arc<tokio::sync::Notify>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool).await?;
    listener.listen("plant_changed").await?;
    tracing::info!("[listen] Listening on plant_changed");

    loop {
        let _notification = listener.recv().await?;
        tracing::info!("[listen] plant_changed — refreshing connectors and wires");

        if let Err(e) = orchestration::refresh_wires(&pool, &wires).await {
            tracing::warn!("[listen] Failed to refresh wire table: {}", e);
        }

        match db::plant_loader::load(&pool).await {
            Ok((plant_config, _)) => {
                for plc in &plant_config.plcs {
                    plant::add_plc_connector(
                        plc, app.tick_ms,
                        Arc::clone(&ingested),
                        Arc::clone(&discovered),
                        Arc::clone(&write_handles),
                        app.pki_dir.clone(),
                        app.opcua_uri_override.clone(),
                    ).await;
                }
            }
            Err(e) => tracing::warn!("[listen] Failed to reload plant from DB: {}", e),
        }

        // Nudge the k8s reconciler so a newly-added simulated PLC gets its pod now,
        // not on the next periodic cycle. Coalesced: a burst of edits (plc + instances
        // + wires all fire plant_changed) collapses to at most one extra reconcile.
        reconcile_kick.notify_one();
    }
}

// ── Fallback wire table (no-DB mode) ─────────────────────────────────────────

fn build_wire_table_from_plant(
    plant: &plant_config::PlantConfig,
) -> Vec<db::plant_loader::OrchestratorWire> {
    let mut wires = Vec::new();
    for plc in &plant.plcs {
        for device in &plc.devices {
            for input in &device.input_variables {
                wires.push(db::plant_loader::OrchestratorWire {
                    src_device_id: input.source_device_id.clone(),
                    src_field:     input.source_field.clone(),
                    dst_plc_name:  plc.name.clone(),
                    dst_node_id:   format!("ns=2;s={}.{}.{}", plc.name, device.device_id, input.name),
                });
            }
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
