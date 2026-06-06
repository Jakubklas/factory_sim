use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use async_trait::async_trait;
use plant_config::DataType;

/// Full shared state written by all connectors: device_id → field_name → value.
pub type IngestedState = HashMap<String, HashMap<String, DataType>>;
/// One connector's poll result — upserted into IngestedState each tick.
pub type PartialState  = HashMap<String, HashMap<String, DataType>>;

/// Implement this to add a new protocol. connect() makes a single attempt; GenericConnector handles retry.
#[async_trait]
pub trait ConnectorImpl: Send + Sync + 'static {
    type Conn: Send + 'static;
    async fn connect(&self) -> Result<Self::Conn, Box<dyn std::error::Error + Send + Sync>>;
    /// Return Err if the connection is broken — triggers reconnect in GenericConnector.
    async fn poll(&self, conn: &Self::Conn) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>>;
}

/// One tokio task per connector. Runs connect → poll loop with exponential backoff on initial connect.
/// async-opcua sessions reconnect internally on transient drops, so explicit reconnect is only
/// needed when the session itself becomes permanently unrecoverable.
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

    /// Spawn a tokio task. Consumes self — ownership moves into the task.
    pub fn start(self) {
        tokio::spawn(async move { self.run().await });
    }

    async fn connect_with_backoff(&self) -> C::Conn {
        let mut attempt: usize = 0;
        loop {
            match self.impl_.connect().await {
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
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn run(self) {
        let mut conn = self.connect_with_backoff().await;
        let mut consecutive_failures: usize = 0;
        let summary_every = Duration::from_secs(30);
        let mut last_summary = Instant::now();

        loop {
            tokio::time::sleep(Duration::from_millis(self.tick_ms)).await;

            match self.impl_.poll(&conn).await {
                Ok(partial) => {
                    consecutive_failures = 0;

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
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    // Session auto-reconnects internally; only re-establish the outer Conn when
                    // failures persist long enough to suggest the session event loop gave up.
                    if consecutive_failures >= self.backoff_secs.len() {
                        conn = self.connect_with_backoff().await;
                        consecutive_failures = 0;
                    }
                }
            }
        }
    }
}
