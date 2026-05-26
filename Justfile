set dotenv-load

config_dir := justfile_directory() + "/config"

# Start the backend (API + connectors)
be:
    PLANT_CONFIG={{config_dir}} cargo run -p backend

# Start one simulator process for a specific PLC.   Usage: just sim plc_001
sim PLC:
    SIM_PLC_ID={{PLC}} PLANT_CONFIG={{config_dir}} cargo run -p simulator

# Spawn one simulator process per simulated PLC in plant.json (parallel)
sim-all:
    cargo build -p simulator
    jq -r '.plcs[] | select(.simulated) | .plc_id' {{config_dir}}/plant.json \
      | xargs -I{} -P0 env SIM_PLC_ID={} PLANT_CONFIG={{config_dir}} \
        ./target/debug/simulator

# ── Docker ────────────────────────────────────────────────────────────────────

# Generate docker-compose.yml from plant.json (requires PLANT_CONFIG or uses ./config)
compose-gen:
    PLANT_CONFIG={{config_dir}} cargo run -p tools --bin gen_compose > docker-compose.yml
    @echo "docker-compose.yml written"

# Build all container images
docker-build:
    docker compose build

# Start the full stack in detached mode
docker-up:
    docker compose up -d

# Stop and remove containers (volumes preserved)
docker-down:
    docker compose down

# Stream logs for a specific service.   Usage: just docker-logs backend
docker-logs SVC:
    docker compose logs -f {{SVC}}
