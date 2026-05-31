use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use plant_config::DataType;

/// Full shared state written by all connectors: device_id → field_name → value.
pub type IngestedState = HashMap<String, HashMap<String, DataType>>;
/// One connector's poll result — upserted into IngestedState each tick, leaving other devices untouched.
pub type PartialState  = HashMap<String, HashMap<String, DataType>>;

/// Implement this to add a new protocol. connect() makes a single attempt; GenericConnector handles retry.
pub trait ConnectorImpl: Send + 'static {
    type Conn: Send + 'static;
    fn connect(&self) -> Result<Self::Conn, Box<dyn std::error::Error + Send + Sync>>;
    /// Return Err if the connection is broken — triggers reconnect in GenericConnector.
    fn poll(&self, conn: &Self::Conn) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>>;
}

/// One OS thread per connector. Runs connect → poll loop with exponential backoff on failure.
pub struct GenericConnector<C: ConnectorImpl> {
    name:         String,
    impl_:        C,
    tick_ms:      u64,
    ingested:     Arc<RwLock<IngestedState>>,
    backoff_secs: &'static [u64],
}

impl<C: ConnectorImpl> GenericConnector<C> {
    pub fn new(name: impl Into<String>, impl_: C, tick_ms: u64, ingested: Arc<RwLock<IngestedState>>) -> Self {
        Self { name: name.into(), impl_, tick_ms, ingested, backoff_secs: &[1, 2, 4, 8, 16, 30] }
    }

    /// Spawn the poll thread. Consumes self — ownership moves into the thread.
    pub fn start(self) {
        std::thread::spawn(move || self.run());
    }

    /// Retries connect() with exponential backoff until it succeeds. Logs at DEBUG until first success.
    fn connect_with_backoff(&self) -> C::Conn {
        let mut attempt: usize = 0;
        loop {
            match self.impl_.connect() {
                Ok(conn) => {
                    if attempt == 0 {
                        tracing::info!("Connector '{}' connected", self.name);
                    } else {
                        tracing::info!("Connector '{}' reconnected after {} attempt(s)", self.name, attempt);
                    }
                    return conn;
                }
                Err(e) => {
                    let delay = self.backoff_secs[attempt.min(self.backoff_secs.len() - 1)];
                    tracing::debug!(
                        "Connector '{}' connect attempt {} failed — retrying in {}s: {}",
                        self.name, attempt + 1, delay, e
                    );
                    std::thread::sleep(std::time::Duration::from_secs(delay));
                    attempt += 1;
                }
            }
        }
    }

    fn run(self) {
        let mut conn = self.connect_with_backoff();
        let mut consecutive_failures: usize = 0;
        let summary_every = Duration::from_secs(30);
        let mut last_summary = Instant::now();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(self.tick_ms));

            match self.impl_.poll(&conn) {
                Ok(partial) => {
                    consecutive_failures = 0;

                    // Log a full state snapshot every 30s at INFO level.
                    if last_summary.elapsed() >= summary_every {
                        last_summary = Instant::now();
                        let mut msg = format!(
                            "\n\n  ── {} {}\n",
                            self.name,
                            "─".repeat(48_usize.saturating_sub(self.name.len()))
                        );
                        let mut device_ids: Vec<&String> = partial.keys().collect();
                        device_ids.sort();
                        for device_id in &device_ids {
                            let fields = &partial[*device_id];
                            let mut keys: Vec<&String> = fields.keys().collect();
                            keys.sort();
                            let pairs: Vec<String> = keys.iter()
                                .map(|k| format!("{}={}", k, fields[*k]))
                                .collect();
                            msg.push_str(&format!("  {:20}  {}\n", device_id, pairs.join("  ")));
                        }
                        msg.push('\n');
                        tracing::info!("{}", msg);
                    }

                    tracing::debug!(
                        "Connector '{}' polled {} device(s): [{}]",
                        self.name,
                        partial.len(),
                        partial.keys().cloned().collect::<Vec<_>>().join(", ")
                    );
                    if let Ok(mut state) = self.ingested.try_write() {
                        for (device_id, fields) in partial {
                            state.entry(device_id).or_default().extend(fields);
                        }
                    }
                }
                Err(e) => {
                    consecutive_failures += 1;
                    let delay = self.backoff_secs[consecutive_failures.min(self.backoff_secs.len() - 1)];
                    tracing::warn!(
                        "Connector '{}' poll failed (attempt {}) — waiting {}s before reconnect: {}",
                        self.name, consecutive_failures, delay, e
                    );
                    std::thread::sleep(std::time::Duration::from_secs(delay));
                    // Drop old connection BEFORE creating the new one — opcua 0.12 uses a shared
                    // async runtime; dropping Client while a new session is being initialised
                    // closes the shared sender and kills the new session's background tasks.
                    drop(conn);
                    conn = self.connect_with_backoff();
                }
            }
        }
    }
}
