use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Lightweight running counters for the periodic simulator summary.
/// All fields are monotonic since process start; cheap relaxed atomics.
#[derive(Default)]
pub struct SimStats {
    ticks:        AtomicU64, // physics-loop iterations
    physics_runs: AtomicU64, // device physics executions
    skipped:      AtomicU64, // devices skipped by the readiness gate
    errors:       AtomicU64, // physics errors
    writes_in:    AtomicU64, // OPC-UA input writes received from the backend
}

impl SimStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one completed tick and its per-device outcome counts.
    pub fn record_tick(&self, computed: u32, skipped: u32, errors: u32) {
        self.ticks.fetch_add(1, Relaxed);
        self.physics_runs.fetch_add(computed as u64, Relaxed);
        self.skipped.fetch_add(skipped as u64, Relaxed);
        self.errors.fetch_add(errors as u64, Relaxed);
    }

    /// Record one OPC-UA write delivered to an input node.
    pub fn record_write(&self) {
        self.writes_in.fetch_add(1, Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            ticks:        self.ticks.load(Relaxed),
            physics_runs: self.physics_runs.load(Relaxed),
            skipped:      self.skipped.load(Relaxed),
            errors:       self.errors.load(Relaxed),
            writes_in:    self.writes_in.load(Relaxed),
        }
    }
}

pub struct StatsSnapshot {
    pub ticks:        u64,
    pub physics_runs: u64,
    pub skipped:      u64,
    pub errors:       u64,
    pub writes_in:    u64,
}
