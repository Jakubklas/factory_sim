use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod loader;
mod state;
mod physics_definitions;
mod functions;
mod tick;
mod port_guard;
mod server;

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

    // Only serve PLCs marked as simulated
    let sim_plcs: Vec<_> = plant.config.plcs.iter()
        .filter(|p| p.simulated)
        .cloned()
        .collect();

    if sim_plcs.is_empty() {
        tracing::warn!("No simulated PLCs found in config — nothing to do");
        return Ok(());
    }

    tracing::info!(
        "\n\n  ══════════════════════════════════\n  simulator  starting\n  ══════════════════════════════════\n\n  \
         tick: {}ms  ·  {} simulated PLC{}\n",
        tick_ms,
        sim_plcs.len(),
        if sim_plcs.len() == 1 { "" } else { "s" },
    );

    // Release ports before binding
    let sim_ports: Vec<u16> = sim_plcs.iter().map(|p| p.port).collect();
    port_guard::release_ports(&sim_ports);

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

    // Start one OPC-UA server per simulated PLC
    for plc in sim_plcs {
        server::plc_server::start_one(
            Arc::clone(&state),
            Arc::clone(&devices),
            plc,
            tick_ms,
        ).await?;
    }

    tracing::info!("\n  ══ Running — press Ctrl-C to stop ══\n");
    tokio::signal::ctrl_c().await?;

    tracing::info!("\n  Ctrl-C received — shutting down");
    port_guard::release_ports(&sim_ports);
    tracing::info!("  Shutdown complete\n");

    Ok(())
}
