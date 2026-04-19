use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::models::DataType;

// Full shared state, written to by all connectors. Layout: device_id → field_name → value
pub type IngestedState = HashMap<String, HashMap<String, DataType>>;
// One connector's poll result — upserted into IngestedState each tick, leaving other devices untouched.
pub type PartialState  = HashMap<String, HashMap<String, DataType>>;

// ============================================================================
// ConnectorImpl — implement this to add a new protocol
// ============================================================================

/// Protocol behaviour only — connect and poll.
/// Identity (name) lives on GenericConnector, not here.
pub trait ConnectorImpl: Send + 'static {
    /// The live connection handle (e.g. a session, a client, a pool).
    /// Must be `Send` so the runner thread can own it.
    type Conn: Send + 'static;

    /// Open a fresh connection. Called once at startup and on reconnect.
    /// Backoff and retry are handled by GenericConnector — make a single attempt here.
    fn connect(&self) -> Result<Self::Conn, Box<dyn std::error::Error + Send + Sync>>;

    /// Read all values for this endpoint and return them as a partial state.
    /// Return `Err` if the connection is broken (triggers reconnect).
    /// Partial failures within a poll should be handled internally.
    fn poll(&self, conn: &Self::Conn) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>>;
}

// ============================================================================
// GenericConnector — one OS thread per connector
// ============================================================================

// C is the concrete connector type (e.g. ScadaPlcConnector) — resolved at compile time.
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

    fn connect_with_backoff(&self) -> C::Conn {
        let mut attempt: usize = 0;
        loop {
            match self.impl_.connect() {
                Ok(conn) => {
                    if attempt > 0 {
                        tracing::info!("Connector '{}' reconnected", self.name);
                    }
                    return conn;
                }
                Err(e) => {
                    let delay = self.backoff_secs[attempt.min(self.backoff_secs.len() - 1)];
                    tracing::warn!(
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
        tracing::info!("Connector '{}' starting", self.name);
        let mut conn = self.connect_with_backoff();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(self.tick_ms));

            match self.impl_.poll(&conn) {
                Ok(partial) => {
                    tracing::debug!(
                        "Connector '{}' polled {} device(s): [{}]",
                        self.name,
                        partial.len(),
                        partial.keys().cloned().collect::<Vec<_>>().join(", ")
                    );
                    // Partial write: only overwrite keys owned by this connector.
                    // Other connectors' device entries are left untouched.
                    if let Ok(mut state) = self.ingested.try_write() {
                        for (device_id, fields) in partial {
                            state.entry(device_id).or_default().extend(fields);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Connector '{}' poll failed — reconnecting: {}",
                        self.name, e
                    );
                    conn = self.connect_with_backoff();
                }
            }
        }
    }
}
