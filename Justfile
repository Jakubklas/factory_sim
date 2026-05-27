set dotenv-load

config_dir  := justfile_directory() + "/config"
deploy_key  := env_var_or_default("DEPLOY_KEY", "~/.ssh/factory_sim_ec2")
deploy_user := env_var_or_default("DEPLOY_USER", "ec2-user")
registry    := env_var_or_default("IMAGE_REGISTRY", "ghcr.io/jakubklas/factory_sim")

# ── Dev ───────────────────────────────────────────────────────────────────────

# Run the backend (API + connectors)
be:
    PLANT_CONFIG={{config_dir}} cargo run -p backend

# Run the simulator for one PLC.  Usage: just sim plc_001
sim PLC:
    SIM_PLC_ID={{PLC}} PLANT_CONFIG={{config_dir}} cargo run -p simulator

# Run one simulator process per simulated PLC in plant.json (parallel)
sim-all:
    cargo build -p simulator
    jq -r '.plcs[] | select(.simulated) | .plc_id' {{config_dir}}/plant.json \
      | xargs -I{} -P0 env SIM_PLC_ID={} PLANT_CONFIG={{config_dir}} \
        ./target/debug/simulator

# ── Docker ────────────────────────────────────────────────────────────────────

# Regenerate docker-compose.yml from plant.json
compose-gen:
    IMAGE_REGISTRY={{registry}} PLANT_CONFIG={{config_dir}} \
        cargo run -p codegen --bin gen_compose > docker-compose.yml
    @echo "docker-compose.yml written"

# Build all container images locally
docker-build:
    just compose-gen
    docker compose build

# Start the full stack locally in detached mode
docker-up:
    docker compose up -d

# Stop and remove containers (volumes preserved)
docker-down:
    docker compose down

# Stream logs for a service.  Usage: just docker-logs backend
docker-logs SVC:
    docker compose logs -f {{SVC}}

# Build, push all images to GHCR.  Requires: gh auth login + write:packages scope
push:
    just compose-gen
    gh auth token | docker login ghcr.io -u jakubklas --password-stdin
    docker compose build
    docker compose push

# ── Deploy ────────────────────────────────────────────────────────────────────

# Bootstrap a fresh Linux host and start the stack.  Usage: just deploy 1.2.3.4
# Override SSH key/user: DEPLOY_KEY=~/.ssh/other.pem DEPLOY_USER=ubuntu just deploy 1.2.3.4
deploy HOST:
    ssh -i {{deploy_key}} -o StrictHostKeyChecking=no {{deploy_user}}@{{HOST}} 'bash -s' \
        < {{justfile_directory()}}/deploy/bootstrap.sh

# Pull latest images and restart the stack on a running host.  Usage: just redeploy 1.2.3.4
redeploy HOST:
    ssh -i {{deploy_key}} -o StrictHostKeyChecking=no {{deploy_user}}@{{HOST}} \
        'cd ~/factory_sim && git pull --ff-only && sudo docker compose pull && sudo docker compose up -d'

# Stream live logs from a deployed host.  Usage: just remote-logs 1.2.3.4
remote-logs HOST:
    ssh -i {{deploy_key}} -o StrictHostKeyChecking=no {{deploy_user}}@{{HOST}} \
        'cd ~/factory_sim && sudo docker compose logs -f'
