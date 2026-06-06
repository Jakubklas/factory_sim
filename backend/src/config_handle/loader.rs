use std::sync::Arc;
use plant_config::{ResolvedPlant, loader as pc_loader};
use super::AppConfig;

/// Load app config from env vars and plant topology from PLANT_CONFIG directory.
/// Returns the app config and an immutable resolved plant.
#[allow(dead_code)]
pub fn load_all() -> Result<(AppConfig, Arc<ResolvedPlant>), Box<dyn std::error::Error>> {
    let app = AppConfig::from_env()?;

    let config_dir = std::env::var("PLANT_CONFIG")
        .map_err(|_| "PLANT_CONFIG env var is not set (expected path to directory containing plant.json)")?;
    let config_dir = std::path::Path::new(&config_dir);

    tracing::info!("Loading config from {}", config_dir.display());

    let plant_config = pc_loader::load_plant_config(config_dir.join("plant.json").to_str().unwrap())?;
    let device_types = pc_loader::load_device_types(config_dir.join("device_types.json").to_str().unwrap())?;
    let plant        = Arc::new(ResolvedPlant::build(plant_config, device_types)?);

    Ok((app, plant))
}
