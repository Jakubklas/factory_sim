use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod loader;
mod state;
mod physics_definitions;
mod functions;
mod tick;
mod server;
mod health;

use state::SimulatorState;
use physics_definitions::PhysicsEngine;
use tick::{TickPlan, tick};

struct SecondsTimer;
impl tracing_subscriber::fmt::time::FormatTime for SecondsTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::from_default_env()
        .add_directive("info".parse()?)
        .add_directive("opcua=warn".parse()?);

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_timer(SecondsTimer))
        .init();

    let tick_ms = loader::tick_ms();
    let plant   = loader::load()?;

    // After slicing, exactly one PLC remains — the one this process owns.
    let plc = plant.config.plcs[0].clone();

    tracing::info!(
        "\n\n  ══════════════════════════════════\n  simulator  starting\n  ══════════════════════════════════\n\n  \
         PLC: {} ({})  ·  tick: {}ms  ·  {} device(s)\n",
        plc.plc_id, plc.name, tick_ms, plant.devices.len(),
    );

    // Build physics engine from all device types
    let device_types: Vec<_> = plant.devices.iter()
        .map(|d| d.type_def.clone())
        .collect();
    let physics = Arc::new(PhysicsEngine::new(&device_types)?);

    // Build tick plan and seed state
    let plan    = Arc::new(TickPlan::build(&plant.devices)?);
    let devices = Arc::new(plant.devices);
    let state   = Arc::new(RwLock::new(SimulatorState::new(&devices)));

    tracing::info!("Physics engine ready  ·  tick order: {}", plan.order().join(" → "));

    // Physics tick loop
    {
        let tick_state   = Arc::clone(&state);
        let tick_devices = Arc::clone(&devices);
        let tick_plan    = Arc::clone(&plan);
        let tick_physics = Arc::clone(&physics);

        tokio::spawn(async move {
            let interval = Duration::from_millis(tick_ms);
            loop {
                let tick_start = Instant::now();
                {
                    let mut s = tick_state.write().await;
                    tick(&mut s, &tick_devices, &tick_plan, &tick_physics, interval.as_secs_f64());
                }
                let elapsed = tick_start.elapsed();
                if let Some(remaining) = interval.checked_sub(elapsed) {
                    tokio::time::sleep(remaining).await;
                } else {
                    tracing::warn!("Simulator tick overran by {:?}", elapsed - interval);
                }
            }
        });
    }

    // Health endpoint for k8s liveness probes — runs on SIM_HEALTH_PORT (default 9000)
    let health_port: u16 = std::env::var("SIM_HEALTH_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000);
    health::start(health_port);

    // Start the OPC-UA server for this process's PLC
    server::plc_server::start_one(
        Arc::clone(&state),
        Arc::clone(&devices),
        plc,
        tick_ms,
    ).await?;

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
