use plant_config::{ResolvedPlant, loader as pc_loader};

/// Load the resolved plant sliced to the PLC named in SIM_PLC_ID.
/// PLANT_CONFIG points to the shared config dir; SIM_PLC_ID picks which PLC this process owns.
pub fn load() -> Result<ResolvedPlant, Box<dyn std::error::Error>> {
    let plc_id = std::env::var("SIM_PLC_ID")
        .map_err(|_| "SIM_PLC_ID env var is not set (expected the plc_id this process should simulate)")?;

    let config_dir = std::env::var("PLANT_CONFIG")
        .map_err(|_| "PLANT_CONFIG env var is not set (expected path to directory containing plant.json)")?;
    let config_dir = std::path::Path::new(&config_dir);

    let plant_config = pc_loader::load_plant_config(
        config_dir.join("plant.json").to_str().unwrap()
    )?;
    let device_types = pc_loader::load_device_types(
        config_dir.join("device_types.json").to_str().unwrap()
    )?;

    // Build validates the whole plant (incl. no cross-PLC input_variables), then slice.
    let full = ResolvedPlant::build(plant_config, device_types)?;
    let sliced = full.slice_to_plc(&plc_id)?;

    if !sliced.config.plcs[0].simulated {
        return Err(format!("PLC '{}' is not marked as simulated", plc_id).into());
    }

    Ok(sliced)
}

/// Read SIM_TICK_MS env var, defaulting to 100ms.
pub fn tick_ms() -> u64 {
    std::env::var("SIM_TICK_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
}
