use std::sync::Arc;
use plant_config::{ResolvedPlant, loader as pc_loader};
use super::AppConfig;

/// Load all config files from the `config/` directory adjacent to the binary.
/// Returns the app config and an immutable resolved plant.
pub fn load_all() -> Result<(AppConfig, Arc<ResolvedPlant>), Box<dyn std::error::Error>> {
    let config_dir = std::env::current_exe()?
        .parent()
        .expect("binary has no parent directory")
        .join("config");

    tracing::info!("Loading config from {}", config_dir.display());

    let app          = AppConfig::load(config_dir.join("app.json").to_str().unwrap())?;
    let plant_config = pc_loader::load_plant_config(config_dir.join("plant.json").to_str().unwrap())?;
    let device_types = pc_loader::load_device_types(config_dir.join("device_types.json").to_str().unwrap())?;
    let plant        = Arc::new(ResolvedPlant::build(plant_config, device_types)?);

    Ok((app, plant))
}
