use std::sync::Arc;
use tokio::sync::RwLock;
use super::{AppConfig, DeviceTypeRegistry, PlantRegistry, PlantConfigHandle};

/// Load all config files from the `config/` directory adjacent to the binary.
/// Returns the app config and a fully-resolved plant handle.
pub fn load_all() -> Result<(AppConfig, Arc<RwLock<PlantConfigHandle>>), Box<dyn std::error::Error>> {
    let config_dir = std::env::current_exe()?
        .parent()
        .expect("binary has no parent directory")
        .join("config");

    tracing::info!("Loading config from {}", config_dir.display());

    let app            = AppConfig::load(config_dir.join("app.json").to_str().unwrap())?;
    let type_registry  = DeviceTypeRegistry::load(config_dir.join("device_types.json").to_str().unwrap())?;
    let plant_registry = PlantRegistry::load(config_dir.join("factory.json").to_str().unwrap())?;
    let handle         = PlantConfigHandle::new(type_registry, plant_registry)?;

    Ok((app, handle))
}
