/// Application-level runtime config — ports, hosts, tick rate.
/// Read from env vars: BE_HOST (default 0.0.0.0), BE_PORT (default 3001), BE_TICK_MS (default 100).
#[derive(Debug)]
pub struct AppConfig {
    pub ws_host: String,
    pub ws_port: u16,
    pub tick_ms: u64,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let ws_host = std::env::var("BE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let ws_port = std::env::var("BE_PORT")
            .unwrap_or_else(|_| "3001".to_string())
            .parse::<u16>()
            .map_err(|_| "BE_PORT must be a valid u16")?;
        let tick_ms = std::env::var("BE_TICK_MS")
            .unwrap_or_else(|_| "100".to_string())
            .parse::<u64>()
            .map_err(|_| "BE_TICK_MS must be a valid u64")?;
        Ok(Self { ws_host, ws_port, tick_ms })
    }
}
