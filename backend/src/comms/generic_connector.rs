use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use plant_config::DataType;

pub type IngestedState  = HashMap<String, HashMap<String, DataType>>;
pub type PartialState   = HashMap<String, HashMap<String, DataType>>;

/// One browsed node returned by ConnectorImpl::browse().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowsedNode {
    pub node_id:     String,
    pub browse_name: String,
    pub browse_path: Option<String>,
    pub datatype:    Option<String>,
    pub is_variable: bool,
    pub writable:    bool,
}

/// Shared discovered state: PLC name → browsed nodes. Updated after every (re)connect.
pub type DiscoveredState = HashMap<String, Vec<BrowsedNode>>;

/// A write command delivered to a connector's inbound queue.
#[derive(Debug, Clone)]
pub struct WriteCmd {
    pub node_id: String,
    pub value:   DataType,
}

/// Handle to a connector's write queue. Clone freely — cheap Arc clone.
#[derive(Clone)]
pub struct WriteHandle {
    tx: mpsc::Sender<WriteCmd>,
}

impl WriteHandle {
    /// Enqueue a write. Non-blocking; drops the command if the queue is full.
    pub fn send(&self, cmd: WriteCmd) {
        let _ = self.tx.try_send(cmd);
    }
}

/// Implement this to add a new protocol.
#[async_trait]
pub trait ConnectorImpl: Send + Sync + 'static {
    type Conn: Send + Sync + 'static;

    /// Make a single connection attempt. GenericConnector retries on error.
    async fn connect(&self) -> Result<Self::Conn, Box<dyn std::error::Error + Send + Sync>>;

    /// Browse the address space once after connect. Returns discovered nodes and
    /// rebuilds the internal poll list. Called again on every reconnect.
    async fn browse(&self, conn: &Self::Conn) -> Result<Vec<BrowsedNode>, Box<dyn std::error::Error + Send + Sync>>;

    /// Write a value to a node. Called before poll() each tick.
    async fn write(&self, conn: &Self::Conn, cmd: &WriteCmd) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Read current values from all browsed variable nodes.
    async fn poll(&self, conn: &Self::Conn) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>>;
}

/// One tokio task per connector. Runs connect → browse → (drain writes → poll) loop.
pub struct GenericConnector<C: ConnectorImpl> {
    name:         String,
    impl_:        C,
    tick_ms:      u64,
    ingested:     Arc<RwLock<IngestedState>>,
    discovered:   Arc<RwLock<DiscoveredState>>,
    write_rx:     mpsc::Receiver<WriteCmd>,
    backoff_secs: &'static [u64],
}

impl<C: ConnectorImpl> GenericConnector<C> {
    /// Returns `(connector, write_handle)`. The caller keeps the handle to enqueue writes.
    pub fn new(
        name:       impl Into<String>,
        impl_:      C,
        tick_ms:    u64,
        ingested:   Arc<RwLock<IngestedState>>,
        discovered: Arc<RwLock<DiscoveredState>>,
    ) -> (Self, WriteHandle) {
        let (tx, rx) = mpsc::channel(256);
        let connector = Self {
            name: name.into(), impl_, tick_ms, ingested, discovered,
            write_rx: rx, backoff_secs: &[1, 2, 4, 8, 16, 30],
        };
        (connector, WriteHandle { tx })
    }

    /// Consume the connector, spawning its run loop as a tokio task.
    pub fn spawn(self) {
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

    async fn do_browse(&self, conn: &C::Conn) {
        match self.impl_.browse(conn).await {
            Ok(nodes) => {
                tracing::info!("Connector '{}' discovered {} node(s)", self.name, nodes.len());
                self.discovered.write().await.insert(self.name.clone(), nodes);
            }
            Err(e) => tracing::warn!("Connector '{}' browse failed: {}", self.name, e),
        }
    }

    async fn run(mut self) {
        let mut conn = self.connect_with_backoff().await;
        self.do_browse(&conn).await;

        let mut consecutive_failures: usize = 0;
        let summary_every = Duration::from_secs(30);
        let mut last_summary = Instant::now();

        loop {
            tokio::time::sleep(Duration::from_millis(self.tick_ms)).await;

            // Drain the write queue before reading — ensures setpoints land before next poll.
            while let Ok(cmd) = self.write_rx.try_recv() {
                if let Err(e) = self.impl_.write(&conn, &cmd).await {
                    tracing::warn!("Connector '{}' write '{}' failed: {}", self.name, cmd.node_id, e);
                }
            }

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
                        "Connector '{}' poll failed (attempt {}) — waiting {}s: {}",
                        self.name, consecutive_failures, delay, e
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    if consecutive_failures >= self.backoff_secs.len() {
                        conn = self.connect_with_backoff().await;
                        self.do_browse(&conn).await;
                        consecutive_failures = 0;
                    }
                }
            }
        }
    }
}
