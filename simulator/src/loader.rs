use plant_config::{ResolvedPlant, DeviceTypeDefinition, loader as pc_loader};
use crate::config::SimConfig;

/// Load the resolved plant for this simulator process.
///
/// Priority:
///   1. If BACKEND_URL is set: fetch config from the backend API.
///      The backend returns {plc, device_types} for the given SIM_PLC_ID.
///   2. Otherwise: read plant.json + device_types.json from PLANT_CONFIG dir.
///
/// In both cases the result is a ResolvedPlant sliced to exactly one PLC.
pub async fn load(cfg: &SimConfig) -> Result<ResolvedPlant, Box<dyn std::error::Error>> {
    if let Some(backend_url) = &cfg.backend_url {
        load_from_api(backend_url, &cfg.plc_id).await
    } else {
        load_from_files(cfg)
    }
}

// ============================================================================
// File-based path (backward compat — used when no BACKEND_URL is set)
// ============================================================================

fn load_from_files(cfg: &SimConfig) -> Result<ResolvedPlant, Box<dyn std::error::Error>> {
    let config_dir = cfg.plant_config_dir.as_deref()
        .ok_or("PLANT_CONFIG env var not set (set BACKEND_URL or PLANT_CONFIG)")?;
    let plc_id     = &cfg.plc_id;
    let config_dir = std::path::Path::new(config_dir);

    let plant_config = pc_loader::load_plant_config(
        config_dir.join("plant.json").to_str().unwrap()
    )?;
    let device_types = pc_loader::load_device_types(
        config_dir.join("device_types.json").to_str().unwrap()
    )?;

    let full   = ResolvedPlant::build(plant_config, device_types)?;
    let sliced = full.slice_to_plc(plc_id)?;

    if !sliced.config.plcs[0].simulated {
        return Err(format!("PLC '{}' is not marked as simulated", plc_id).into());
    }

    Ok(sliced)
}

// ============================================================================
// API-based path — GET {BACKEND_URL}/api/plcs/{SIM_PLC_ID}/config
// Retries with exponential backoff so the sim can start before the backend is ready.
// ============================================================================

#[derive(serde::Deserialize)]
struct ApiConfigResponse {
    plc:          plant_config::PlcConfig,
    device_types: Vec<DeviceTypeDefinition>,
}

async fn load_from_api(
    backend_url: &str,
    plc_id:      &str,
) -> Result<ResolvedPlant, Box<dyn std::error::Error>> {
    use http_body_util::{BodyExt, Empty};
    use hyper::body::Bytes;
    use hyper::Request;
    use hyper_util::client::legacy::Client;
    use hyper_util::rt::TokioExecutor;

    let url_str = format!("{}/api/plcs/{}/config", backend_url.trim_end_matches('/'), plc_id);
    let uri: hyper::Uri = url_str.parse()
        .map_err(|e| format!("invalid backend URL '{}': {}", url_str, e))?;

    tracing::info!("Fetching simulator config from {}", url_str);

    let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();

    // Exponential backoff: 2s, 4s, 8s, 16s, 30s, 30s, ...
    let backoff_secs: &[u64] = &[2, 4, 8, 16, 30, 30];
    let mut attempt = 0usize;

    loop {
        let req = Request::builder()
            .method("GET")
            .uri(uri.clone())
            .header("accept", "application/json")
            .body(Empty::new())?;

        match client.request(req).await {
            Ok(resp) if resp.status().is_success() => {
                let body_bytes = resp.into_body().collect().await?.to_bytes();
                let api_resp: ApiConfigResponse = serde_json::from_slice(&body_bytes)
                    .map_err(|e| format!("failed to parse config response: {}", e))?;

                // Reconstruct a minimal PlantConfig from the single PlcConfig returned.
                let plant_name = api_resp.plc.name.clone();
                let plant_config = plant_config::PlantConfig {
                    plant_id:    plc_id.to_string(),
                    name:        plant_name,
                    description: String::new(),
                    plcs:        vec![api_resp.plc],
                };

                let plant = ResolvedPlant::build(plant_config, api_resp.device_types)?;

                if !plant.config.plcs[0].simulated {
                    return Err(format!("PLC '{}' from backend is not simulated", plc_id).into());
                }

                tracing::info!("Config loaded from backend API  ({} device(s))", plant.devices.len());
                return Ok(plant);
            }
            Ok(resp) => {
                tracing::warn!("Backend returned {} for config — will retry", resp.status());
            }
            Err(e) => {
                tracing::warn!("Backend not reachable ({}), will retry", e);
            }
        }

        let delay = backoff_secs.get(attempt).copied().unwrap_or(30);
        tracing::info!("Retrying in {}s…", delay);
        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
        attempt += 1;
    }
}
