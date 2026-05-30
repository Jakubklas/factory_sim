// Generate a device-specific docker-compose.yml
// Usage: DEVICE=pi cargo run -p codegen --bin gen_compose_device
//
// Only includes services that should run on the specified device based on:
// - plant.json (PLCs with deploy_target matching DEVICE)
// - platform.json (backend/frontend with deploy_target matching DEVICE)

use std::fmt::Write as _;
use plant_config::loader::load_plant_config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = std::env::var("PLANT_CONFIG")
        .unwrap_or_else(|_| "./config".to_string());
    let registry = std::env::var("IMAGE_REGISTRY")
        .unwrap_or_else(|_| "ghcr.io/jakubklas/factory_sim".to_string());
    let device = std::env::var("DEVICE")
        .expect("DEVICE environment variable required");
    
    // Load inventory to get device architecture
    let inventory_path = format!("{}/deploy/inventory.json", std::env::current_dir()?.display());
    let inventory_json = std::fs::read_to_string(&inventory_path)?;
    let inventory: serde_json::Value = serde_json::from_str(&inventory_json)?;
    let device_arch = inventory["devices"][&device]["architecture"]
        .as_str()
        .unwrap_or("amd64");
    
    // Use latest tag for all architectures (multi-arch handled by Docker registry)
    let image_tag = "latest";

    // Load configurations
    let plant_path = format!("{}/plant.json", config_dir);
    let plant = load_plant_config(&plant_path)?;
    
    let platform_path = format!("{}/deploy/platform.json", std::env::current_dir()?.display());
    let platform_json = std::fs::read_to_string(&platform_path)?;
    let platform: serde_json::Value = serde_json::from_str(&platform_json)?;

    // Filter services for this device
    let device_plcs: Vec<_> = plant.plcs.iter()
        .filter(|p| p.simulated && p.deploy_target.as_ref() == Some(&device))
        .collect();
    
    let should_run_backend = platform["components"]["backend"]["deploy_target"] == device;
    let should_run_frontend = platform["components"]["frontend"]["deploy_target"] == device;

    let mut out = String::new();

    writeln!(out, "# Device-specific compose for: {}", device)?;
    writeln!(out, "# Auto-generated — do not edit by hand")?;
    writeln!(out)?;
    writeln!(out, "services:")?;

    // PLCs for this device
    for plc in &device_plcs {
        writeln!(out, "  {}:", plc.plc_id)?;
        writeln!(out, "    build:")?;
        writeln!(out, "      context: .")?;
        writeln!(out, "      dockerfile: simulator/Dockerfile")?;
        writeln!(out, "    image: {}/simulator:{}", registry, image_tag)?;
        writeln!(out, "    environment:")?;
        writeln!(out, "      PLANT_CONFIG: /config")?;
        writeln!(out, "      SIM_PLC_ID: {}", plc.plc_id)?;
        writeln!(out, "      SIM_TICK_MS: 100")?;
        writeln!(out, "      OPCUA_HOST: {}", plc.plc_id)?;
        writeln!(out, "      PKI_DIR: /data/pki")?;
        writeln!(out, "    volumes:")?;
        writeln!(out, "      - ./config:/config:ro")?;
        writeln!(out, "      - {}_data:/data", plc.plc_id)?;
        writeln!(out, "    ports:")?;
        writeln!(out, "      - \"{}:{}\"", plc.port, plc.port)?;
        writeln!(out, "    restart: unless-stopped")?;
        writeln!(out)?;
    }

    // Backend (if assigned to this device)
    if should_run_backend {
        writeln!(out, "  backend:")?;
        writeln!(out, "    build:")?;
        writeln!(out, "      context: .")?;
        writeln!(out, "      dockerfile: backend/Dockerfile")?;
        writeln!(out, "    image: {}/backend:{}", registry, image_tag)?;
        writeln!(out, "    environment:")?;
        writeln!(out, "      PLANT_CONFIG: /config")?;
        writeln!(out, "      BE_HOST: 0.0.0.0")?;
        writeln!(out, "      BE_PORT: 3001")?;
        writeln!(out, "      BE_TICK_MS: 100")?;
        writeln!(out, "      PKI_DIR: /data/pki")?;
        writeln!(out, "    volumes:")?;
        writeln!(out, "      - ./config:/config:ro")?;
        writeln!(out, "      - backend_data:/data")?;
        writeln!(out, "    ports:")?;
        writeln!(out, "      - \"3001:3001\"")?;
        writeln!(out, "    healthcheck:")?;
        writeln!(out, "      test: [\"CMD-SHELL\", \"wget -qO- http://localhost:3001/health || exit 1\"]")?;
        writeln!(out, "      interval: 10s")?;
        writeln!(out, "      timeout: 5s")?;
        writeln!(out, "      retries: 5")?;
        writeln!(out, "    restart: unless-stopped")?;
        writeln!(out)?;
    }

    // Frontend (if assigned to this device)
    if should_run_frontend {
        writeln!(out, "  frontend:")?;
        writeln!(out, "    build:")?;
        writeln!(out, "      context: frontend")?;
        writeln!(out, "      dockerfile: Dockerfile")?;
        writeln!(out, "    image: {}/frontend:{}", registry, image_tag)?;
        writeln!(out, "    environment:")?;
        writeln!(out, "      BE_URL: ${{BE_URL:-http://localhost:3001}}")?;
        writeln!(out, "    ports:")?;
        writeln!(out, "      - \"8080:80\"")?;
        if should_run_backend {
            writeln!(out, "    depends_on:")?;
            writeln!(out, "      - backend")?;
        }
        writeln!(out, "    restart: unless-stopped")?;
        writeln!(out)?;
    }

    // Named volumes
    if !device_plcs.is_empty() || should_run_backend {
        writeln!(out, "volumes:")?;
        for plc in &device_plcs {
            writeln!(out, "  {}_data:", plc.plc_id)?;
        }
        if should_run_backend {
            writeln!(out, "  backend_data:")?;
        }
    }

    print!("{}", out);
    Ok(())
}