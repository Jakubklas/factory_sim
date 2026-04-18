// Reference implementation — shows how to wire a non-OPC-UA connector.
// Requires: tokio-postgres = "0.7" in Cargo.toml
//
// use tokio_postgres::{Client, NoTls};

use std::collections::HashMap;
use crate::models::DataType;
use crate::comms::generic_connector::{ConnectorImpl, PartialState};

// ============================================================================
// Connection handle
// ============================================================================

pub struct PgConnection {
    // client: tokio_postgres::Client,
}

// ============================================================================
// PostgresConnector — one instance per database endpoint
// ============================================================================

pub struct PostgresConnector {
    connection_string: String,
    // Which device_ids this connector is responsible for.
    // Prevents two connectors overwriting each other's keys in IngestedState.
    device_ids: Vec<String>,
}

impl PostgresConnector {
    pub fn new(connection_string: impl Into<String>, device_ids: Vec<String>) -> Self {
        Self { connection_string: connection_string.into(), device_ids }
    }
}

impl ConnectorImpl for PostgresConnector {
    type Conn = PgConnection;

    fn connect(&self) -> Result<PgConnection, Box<dyn std::error::Error + Send + Sync>> {
        // let (client, connection) = tokio_postgres::connect(&self.connection_string, NoTls).await?;
        // tokio::spawn(async move { connection.await });
        // Ok(PgConnection { client })
        todo!("open postgres connection")
    }

    fn poll(&self, _conn: &PgConnection) -> Result<PartialState, Box<dyn std::error::Error + Send + Sync>> {
        // let rows = conn.client
        //     .query("SELECT device_id, field, value FROM live_state WHERE device_id = ANY($1)", &[&self.device_ids])
        //     .await?;
        //
        // let mut partial = PartialState::new();
        // for row in rows {
        //     let device_id: String = row.get("device_id");
        //     let field:     String = row.get("field");
        //     let value:     f64   = row.get("value");
        //     partial.entry(device_id).or_default().insert(field, DataType::Float(value));
        // }
        // Ok(partial)
        todo!("query live_state table and map rows into PartialState")
    }
}
