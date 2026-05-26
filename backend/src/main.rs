use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt, reload};

/// Formats log timestamps as seconds-resolution ISO-8601 (e.g. 2026-05-03T17:00:00Z).
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

use config_handle::load_all;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Default to info; suppress noisy opcua internals to warn. Reloadable at runtime via /api/log-level.
    let filter = EnvFilter::from_default_env()
        .add_directive("info".parse()?)
        .add_directive("opcua=warn".parse()?);

    let (filter_layer, log_handle) = reload::Layer::new(filter);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer().with_timer(SecondsTimer))
        .init();

    // Load app.json (ports, tick rate) + plant.json/device_types.json (resolved plant topology).
    let (app, plant) = load_all()?;

    let addr = format!("{}:{}", app.ws_host, app.ws_port);
    tracing::info!(
        "\n\n  ══════════════════════════════════\n  water-plant-twin  starting\n  ══════════════════════════════════\n\n  \
         API  {}\n\n    \
         ws://{}/ws\n    \
         http://{}/api/plant\n    \
         http://{}/api/log-level?set=info\n    \
         http://{}/api/log-level?set=debug\n    \
         http://{}/api/log-level?set=backend::comms=trace\n",
        addr, addr, addr, addr, addr, addr
    );

    // Start one OPC-UA connector per PLC; returns the shared ingested-state map.
    let ingested    = plant::start(plant.clone(), app.tick_ms).await?;
    let ingested_ws = ingested.clone();
    let ws_host     = app.ws_host.clone();

    // WS bridge runs alongside the connectors — serves /ws, /api/plant, /api/log-level.
    tokio::spawn(async move {
        if let Err(e) = api::ws_bridge::start(ingested_ws, plant, app.tick_ms, &ws_host, app.ws_port, log_handle).await {
            tracing::error!("WS bridge error: {}", e);
        }
    });

    tracing::info!("\n  ══ Running — press Ctrl-C or send SIGTERM to stop ══\n");
    shutdown_signal().await;

    tracing::info!("\n  Shutdown signal received — exiting\n");
    Ok(())
}

/// Resolve when SIGINT (Ctrl-C) or SIGTERM (Docker stop) arrives.
/// Installing the SIGTERM handler is essential under Docker: without it the runtime
/// receives the default action (terminate) only after the kernel sends SIGKILL.
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
