/// Central runtime config for the backend.
///
/// This is the **single place** every backend environment variable is read,
/// named, and given a default. Nothing else in the crate should call
/// `std::env::var` directly — load this once at startup and pass it (or the
/// fields it needs) down to the components.
///
/// | Env var             | Field               | Default                        |
/// |---------------------|---------------------|--------------------------------|
/// | `BE_HOST`           | `ws_host`           | `0.0.0.0`                      |
/// | `BE_PORT`           | `ws_port`           | `3001`                         |
/// | `BE_TICK_MS`        | `tick_ms`           | `100`                          |
/// | `DATABASE_URL`      | `database_url`      | unset → no persistence         |
/// | `SEED_DIR`          | `seed_dir`          | unset → no seeding             |
/// | `PLANT_CONFIG`      | `plant_config_dir`  | `/config`                      |
/// | `ASSET_DIR`         | `asset_dir`         | `/data/assets`                 |
/// | `PKI_DIR`           | `pki_dir`           | `./pki`                        |
/// | `OPCUA_URI_OVERRIDE`| `opcua_uri_override`| unset → use each PLC's URI     |
/// | `SIMULATOR_IMAGE`   | `simulator_image`   | `factory-sim/simulator:latest` |
/// | `BACKEND_SVC_URL`   | `backend_svc_url`   | `http://backend:{ws_port}`     |
/// | `K8S_NAMESPACE`     | `k8s_namespace`     | `default`                      |
#[derive(Debug, Clone)]
pub struct AppConfig {
    // ── API / WebSocket server ──
    pub ws_host: String,
    pub ws_port: u16,
    pub tick_ms: u64,
    // ── Persistence / seed / config ──
    pub database_url:     Option<String>,
    pub seed_dir:         Option<String>,
    pub plant_config_dir: String,
    pub asset_dir:        String,
    // ── OPC-UA ──
    pub pki_dir:            String,
    pub opcua_uri_override: Option<String>,
    // ── Kubernetes reconciler ──
    pub simulator_image: String,
    pub backend_svc_url: String,
    pub k8s_namespace:   String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let ws_host = var_or("BE_HOST", "0.0.0.0");
        let ws_port = var_or("BE_PORT", "3001")
            .parse::<u16>()
            .map_err(|_| "BE_PORT must be a valid u16")?;
        let tick_ms = var_or("BE_TICK_MS", "100")
            .parse::<u64>()
            .map_err(|_| "BE_TICK_MS must be a valid u64")?;

        Ok(Self {
            ws_host,
            ws_port,
            tick_ms,
            database_url:     var_opt("DATABASE_URL"),
            seed_dir:         var_opt("SEED_DIR"),
            plant_config_dir: var_or("PLANT_CONFIG", "/config"),
            asset_dir:        var_or("ASSET_DIR", "/data/assets"),
            pki_dir:          var_or("PKI_DIR", "./pki"),
            opcua_uri_override: var_opt("OPCUA_URI_OVERRIDE"),
            simulator_image:  var_or("SIMULATOR_IMAGE", "factory-sim/simulator:latest"),
            backend_svc_url:  var_opt("BACKEND_SVC_URL")
                .unwrap_or_else(|| format!("http://backend:{}", ws_port)),
            k8s_namespace:    var_or("K8S_NAMESPACE", "default"),
        })
    }
}

/// Read an env var, falling back to `default` when unset.
fn var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Read an env var as `Some` only when it's set and non-empty.
fn var_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}
