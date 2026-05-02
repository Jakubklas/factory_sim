use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use crate::config_handle::{PlantConfigHandle, PlcConfig};
use super::{PhysicsEngine, TickPlan, tick};
use super::server::plc_server;

pub struct SimulatorModule;

impl SimulatorModule {
    /// Compile physics scripts and start the global tick loop.
    /// Call once before starting any per-PLC servers.
    /// Fails fast if any script has a syntax error or wiring is cyclic.
    pub async fn start_physics(
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

        tracing::info!("Simulator tick order: {:?}", plan.order());

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

        Ok(())
    }

    /// Start the OPC-UA server for a single simulated PLC.
    pub async fn start_server(
        handle: Arc<RwLock<PlantConfigHandle>>,
        plc:    PlcConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        plc_server::start_one(handle, plc).await
    }
}
