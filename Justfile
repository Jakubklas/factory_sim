set dotenv-load

config_dir  := justfile_directory() + "/config"
deploy_key  := env_var_or_default("DEPLOY_KEY", "~/.ssh/id_ed25519")
deploy_user := env_var_or_default("DEPLOY_USER", "pi")
pi_host     := env_var_or_default("PI_HOST", "100.101.6.122")
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

# ── Device management ─────────────────────────────────────────────────────

# Deploy to a specific device.  Usage: just deploy-device pi
deploy-device DEVICE:
    #!/bin/bash
    set -euo pipefail
    
    # Load device metadata
    DEVICE_DATA=$(jq -r '.devices.{{DEVICE}}' {{justfile_directory()}}/deploy/inventory.json)
    HOST=$(echo "$DEVICE_DATA" | jq -r '.host')
    ENV=$(echo "$DEVICE_DATA" | jq -r '.environment') 
    PROFILE=$(echo "$DEVICE_DATA" | jq -r '.credential_profile')
    
    # Load credentials
    CREDS=$(jq -r '.' {{justfile_directory()}}/deploy/credentials/"$ENV"/"$PROFILE".json)
    SSH_KEY=$(echo "$CREDS" | jq -r '.ssh_key')
    SSH_USER=$(echo "$CREDS" | jq -r '.ssh_user')
    
    echo "Deploying to {{DEVICE}} ($SSH_USER@$HOST)"
    
    # Generate device-specific bootstrap script
    cat {{justfile_directory()}}/deploy/bootstrap.sh | \
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "$SSH_USER@$HOST" \
        "DEPLOY_DEVICE={{DEVICE}} bash -s"

# Redeploy to a specific device.  Usage: just redeploy-device ec2
redeploy-device DEVICE:
    #!/bin/bash
    set -euo pipefail
    
    # Load device metadata and credentials
    DEVICE_DATA=$(jq -r '.devices.{{DEVICE}}' {{justfile_directory()}}/deploy/inventory.json)
    HOST=$(echo "$DEVICE_DATA" | jq -r '.host')
    ENV=$(echo "$DEVICE_DATA" | jq -r '.environment') 
    PROFILE=$(echo "$DEVICE_DATA" | jq -r '.credential_profile')
    
    CREDS=$(jq -r '.' {{justfile_directory()}}/deploy/credentials/"$ENV"/"$PROFILE".json)
    SSH_KEY=$(echo "$CREDS" | jq -r '.ssh_key')
    SSH_USER=$(echo "$CREDS" | jq -r '.ssh_user')
    
    echo "Redeploying to {{DEVICE}} ($SSH_USER@$HOST)"
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "$SSH_USER@$HOST" \
        'cd ~/factory_sim && git pull --ff-only && sudo docker compose pull && sudo docker compose up -d'

# Stream logs from a specific device.  Usage: just logs-device pi
logs-device DEVICE:
    #!/bin/bash
    set -euo pipefail
    
    # Load device metadata and credentials
    DEVICE_DATA=$(jq -r '.devices.{{DEVICE}}' {{justfile_directory()}}/deploy/inventory.json)
    HOST=$(echo "$DEVICE_DATA" | jq -r '.host')
    ENV=$(echo "$DEVICE_DATA" | jq -r '.environment') 
    PROFILE=$(echo "$DEVICE_DATA" | jq -r '.credential_profile')
    
    CREDS=$(jq -r '.' {{justfile_directory()}}/deploy/credentials/"$ENV"/"$PROFILE".json)
    SSH_KEY=$(echo "$CREDS" | jq -r '.ssh_key')
    SSH_USER=$(echo "$CREDS" | jq -r '.ssh_user')
    
    echo "Streaming logs from {{DEVICE}} ($SSH_USER@$HOST)"
    ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no "$SSH_USER@$HOST" \
        'cd ~/factory_sim && sudo docker compose logs -f'

# List all configured devices
list-devices:
    jq -r '.devices | to_entries[] | "\(.key): \(.value.name) (\(.value.host)) - Environment: \(.value.environment)"' \
        {{justfile_directory()}}/deploy/inventory.json

# Deploy all PLCs to their target devices (reads from plant.json)
deploy-plant:
    #!/bin/bash
    set -euo pipefail
    echo "Deploying PLCs to target devices based on plant.json..."
    
    # Get unique deploy targets from plant.json
    TARGETS=$(jq -r '.plcs[] | select(.deploy_target != "local") | .deploy_target' {{config_dir}}/plant.json | sort -u)
    
    for target in $TARGETS; do
        echo "Deploying to device: $target"
        just deploy-device "$target"
    done
    
    echo "Plant deployment complete!"

# Show PLC to device mapping from plant.json
show-deployment:
    jq -r '.plcs[] | "\(.plc_id): \(.name) → \(.deploy_target // "local")"' {{config_dir}}/plant.json

# Deploy platform components (backend, frontend) to their target devices
deploy-platform:
    #!/bin/bash
    set -euo pipefail
    echo "Deploying platform components based on platform.json..."
    
    # Get unique deploy targets from platform.json
    TARGETS=$(jq -r '.components[] | .deploy_target' {{justfile_directory()}}/deploy/platform.json | sort -u)
    
    for target in $TARGETS; do
        echo "Deploying platform components to device: $target"
        just deploy-device "$target"
    done
    
    echo "Platform deployment complete!"

# Deploy everything: platform + PLCs
deploy-all:
    just deploy-platform
    just deploy-plant

# Show platform component to device mapping
show-platform:
    jq -r '.components | to_entries[] | "\(.key): \(.value.description) → \(.value.deploy_target)"' \
        {{justfile_directory()}}/deploy/platform.json
