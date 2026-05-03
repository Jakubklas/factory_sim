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

    let ingested    = plant::start(plant.clone(), app.tick_ms).await?;
    let ingested_ws = ingested.clone();
    let ws_host     = app.ws_host.clone();
    tokio::spawn(async move {
        if let Err(e) = api::ws_bridge::start(ingested_ws, plant, app.tick_ms, &ws_host, app.ws_port, log_handle).await {
            tracing::error!("WS bridge error: {}", e);
        }
    });

    tracing::info!("\n  ══ Running — press Ctrl-C to stop ══\n");
    tokio::signal::ctrl_c().await?;

    tracing::info!("\n  Ctrl-C received — shutting down\n");
    Ok(())
}
