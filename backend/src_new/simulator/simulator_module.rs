use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use super::{PlantConfigHandle, PhysicsEngine, TickPlan, tick};
use super::server::plc_server;

// ============================================================================
// SimulatorModule — self-contained simulator instance
// ============================================================================

/// A running simulator: physics tick loop + one OPC-UA server per PLC.
/// Spawn it when you want to simulate the plant locally.
///
/// The PlantConfigHandle is built externally and shared — the simulator binds
/// OPC-UA servers at the addresses the config specifies. The connector layer
/// reads those same addresses from PlantConfigHandle::endpoint_configs(),
/// so neither side hard-codes the other's concerns.
pub struct SimulatorModule;

impl SimulatorModule {
    /// Start the simulator from an already-built PlantConfigHandle.
    /// Compiles physics scripts, starts the tick loop, starts OPC-UA servers.
    /// Fails fast if any physics script has a syntax error or wiring is cyclic.
    pub async fn spawn(
        handle: Arc<RwLock<PlantConfigHandle>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let physics = {
            let h = handle.read().await;
            let device_types: Vec<_> = h.resolved_devices()
                .iter()
                .map(|d| d.type_def.clone())
                .collect();
            Arc::new(PhysicsEngine::new(&device_types)?)
        };

        let plan = {
            let h = handle.read().await;
            Arc::new(TickPlan::build(&h)?)
        };

        tracing::info!(
            "Simulator tick order: {:?}",
            plan.order()
        );

        // Tick loop — advances physics every tick_ms
        let tick_handle  = Arc::clone(&handle);
        let tick_physics = Arc::clone(&physics);
        let tick_plan    = Arc::clone(&plan);

        tokio::spawn(async move {
            let tick_ms  = tick_handle.read().await.default_tick_ms();
            let interval = Duration::from_millis(tick_ms);
            loop {
                let tick_start = Instant::now();
                {
                    let mut h = tick_handle.write().await;
                    tick(&mut h, &tick_plan, &tick_physics, interval.as_secs_f64());
                }
                let elapsed = tick_start.elapsed();
                if let Some(remaining) = interval.checked_sub(elapsed) {
                    tokio::time::sleep(remaining).await;
                } else {
                    tracing::warn!("Simulator tick overran by {:?}", elapsed - interval);
                }
            }
        });

        // OPC-UA servers — one per PLC, bound at the addresses the config specifies
        plc_server::start(Arc::clone(&handle)).await?;

        let endpoint_count = handle.read().await.all_plcs().len();
        tracing::info!(
            "Simulator started — {} OPC-UA server(s) ready",
            endpoint_count
        );

        Ok(())
    }
}
