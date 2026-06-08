use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod loader;
mod state;
mod physics_definitions;
mod functions;
mod tick;
mod stats;
mod server;
mod health;

use config::SimConfig;
use state::SimulatorState;
use stats::SimStats;
use physics_definitions::PhysicsEngine;
use tick::tick;

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

    let cfg     = SimConfig::from_env()?;
    let tick_ms = cfg.tick_ms;
    let plant   = loader::load(&cfg).await?;

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

    let devices = Arc::new(plant.devices);
    let state   = Arc::new(RwLock::new(SimulatorState::new(&devices)));
    let stats   = Arc::new(SimStats::new());

    tracing::info!("Physics engine ready  ·  Jacobi tick  ·  {} device(s)", devices.len());

    // Physics tick loop
    {
        let tick_state   = Arc::clone(&state);
        let tick_devices = Arc::clone(&devices);
        let tick_physics = Arc::clone(&physics);
        let tick_stats   = Arc::clone(&stats);

        tokio::spawn(async move {
            let interval = Duration::from_millis(tick_ms);
            loop {
                let tick_start = Instant::now();
                let counts = {
                    let mut s = tick_state.write().await;
                    tick(&mut s, &tick_devices, &tick_physics, interval.as_secs_f64())
                };
                tick_stats.record_tick(counts.computed, counts.skipped, counts.errors);
                let elapsed = tick_start.elapsed();
                if let Some(remaining) = interval.checked_sub(elapsed) {
                    tokio::time::sleep(remaining).await;
                } else {
                    tracing::warn!("Simulator tick overran by {:?}", elapsed - interval);
                }
            }
        });
    }

    // Periodic summary — current device values + running counters, every 10s.
    {
        let log_state = Arc::clone(&state);
        let log_stats = Arc::clone(&stats);
        let plc_name  = plc.name.clone();

        tokio::spawn(async move {
            let start = Instant::now();
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            interval.tick().await; // consume the immediate first tick
            loop {
                interval.tick().await;
                log_summary(&plc_name, start, &log_state, &log_stats).await;
            }
        });
    }

    // Health endpoint for k8s liveness probes — runs on SIM_HEALTH_PORT (default 9000)
    health::start(cfg.health_port);

    // Start the OPC-UA server for this process's PLC
    server::plc_server::start_one(
        Arc::clone(&state),
        Arc::clone(&devices),
        plc,
        tick_ms,
        cfg.advertise_host.clone(),
        cfg.pki_dir.clone(),
        Arc::clone(&stats),
    ).await?;

    tracing::info!("\n  ══ Running — press Ctrl-C or send SIGTERM to stop ══\n");
    shutdown_signal().await;

    tracing::info!("\n  Shutdown signal received — exiting\n");
    Ok(())
}

/// Log a periodic summary block: running counters + current value of every device field.
/// Mirrors the backend connector summary so sim and backend logs read the same.
async fn log_summary(
    plc_name: &str,
    start:    Instant,
    state:    &RwLock<SimulatorState>,
    stats:    &SimStats,
) {
    let snapshot = state.read().await.snapshot();
    let s        = stats.snapshot();
    let uptime   = start.elapsed().as_secs().max(1);
    let rate     = s.ticks as f64 / uptime as f64;

    let mut msg = format!(
        "\n\n  ── {} {}\n",
        plc_name,
        "─".repeat(48_usize.saturating_sub(plc_name.len()))
    );
    msg.push_str(&format!(
        "  uptime {}s · ticks {} ({:.1}/s) · physics {} · skipped {} · errors {} · writes-in {}\n",
        uptime, s.ticks, rate, s.physics_runs, s.skipped, s.errors, s.writes_in
    ));

    let mut device_ids: Vec<&String> = snapshot.keys().collect();
    device_ids.sort();
    for device_id in &device_ids {
        let fields = &snapshot[*device_id];
        let mut keys: Vec<&String> = fields.keys().collect();
        keys.sort();
        let pairs: Vec<String> = keys.iter().map(|k| format!("{}={}", k, fields[*k])).collect();
        msg.push_str(&format!("  {:20}  {}\n", device_id, pairs.join("  ")));
    }
    msg.push('\n');
    tracing::info!("{}", msg);
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
