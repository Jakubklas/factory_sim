use plant_config::{ResolvedPlant, loader as pc_loader};

/// Load the resolved plant from the path in PLANT_CONFIG env var.
/// Fails fast if the env var is missing or the files can't be parsed.
pub fn load() -> Result<ResolvedPlant, Box<dyn std::error::Error>> {
    let config_dir = std::env::var("PLANT_CONFIG")
        .map_err(|_| "PLANT_CONFIG env var is not set (expected path to directory containing plant.json)")?;
    let config_dir = std::path::Path::new(&config_dir);

    let plant_config  = pc_loader::load_plant_config(
        config_dir.join("plant.json").to_str().unwrap()
    )?;
    let device_types  = pc_loader::load_device_types(
        config_dir.join("device_types.json").to_str().unwrap()
    )?;

    ResolvedPlant::build(plant_config, device_types)
}

/// Read SIM_TICK_MS env var, defaulting to 100ms.
pub fn tick_ms() -> u64 {
    std::env::var("SIM_TICK_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
}
