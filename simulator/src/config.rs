/// Central runtime config for the simulator.
///
/// The **single place** every simulator environment variable is read, named, and
/// given a default. Nothing else should call `std::env::var` directly.
///
/// | Env var          | Field              | Default                       |
/// |------------------|--------------------|-------------------------------|
/// | `SIM_PLC_ID`     | `plc_id`           | **required**                  |
/// | `BACKEND_URL`    | `backend_url`      | unset → load from files       |
/// | `PLANT_CONFIG`   | `plant_config_dir` | unset → required if no backend|
/// | `SIM_TICK_MS`    | `tick_ms`          | `100`                         |
/// | `SIM_HEALTH_PORT`| `health_port`      | `9000`                        |
/// | `OPCUA_HOST`     | `advertise_host`   | `HOSTNAME`, else `localhost`  |
/// | `PKI_DIR`        | `pki_dir`          | `./pki`                       |
#[derive(Debug, Clone)]
pub struct SimConfig {
    pub plc_id:           String,
    pub backend_url:      Option<String>,
    pub plant_config_dir: Option<String>,
    pub tick_ms:          u64,
    pub health_port:      u16,
    pub advertise_host:   String,
    pub pki_dir:          String,
}

impl SimConfig {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let plc_id = var_opt("SIM_PLC_ID").ok_or("SIM_PLC_ID env var not set")?;

        let advertise_host = var_opt("OPCUA_HOST")
            .or_else(|| var_opt("HOSTNAME"))
            .unwrap_or_else(|| "localhost".to_string());

        Ok(Self {
            plc_id,
            backend_url:      var_opt("BACKEND_URL"),
            plant_config_dir: var_opt("PLANT_CONFIG"),
            tick_ms:          var_opt("SIM_TICK_MS").and_then(|s| s.parse().ok()).unwrap_or(100),
            health_port:      var_opt("SIM_HEALTH_PORT").and_then(|s| s.parse().ok()).unwrap_or(9000),
            advertise_host,
            pki_dir:          var_opt("PKI_DIR").unwrap_or_else(|| "./pki".to_string()),
        })
    }
}

/// Read an env var as `Some` only when it's set and non-empty.
fn var_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}
