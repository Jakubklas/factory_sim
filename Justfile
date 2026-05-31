set dotenv-load

config_dir := justfile_directory() + "/config"
registry   := env_var_or_default("IMAGE_REGISTRY", "ghcr.io/jakubklas/factory_sim")

# ── Dev ───────────────────────────────────────────────────────────────────────

# Run the backend (API + connectors)
be:
    PLANT_CONFIG={{config_dir}} cargo run -p backend

# Run the simulator for one PLC.  Usage: just sim plc-001
sim PLC:
    SIM_PLC_ID={{PLC}} PLANT_CONFIG={{config_dir}} cargo run -p simulator

# Run one simulator process per simulated PLC in plant.json (parallel)
sim-all:
    cargo build -p simulator
    jq -r '.plcs[] | select(.simulated) | .plc_id' {{config_dir}}/plant.json \
      | xargs -I{} -P0 env SIM_PLC_ID={} PLANT_CONFIG={{config_dir}} \
        ./target/debug/simulator

# ── k3s / Helm ────────────────────────────────────────────────────────────────

# Generate helm/factory-sim/values.yaml from plant.json + platform.json + inventory.json
helm-gen:
    IMAGE_REGISTRY={{registry}} PLANT_CONFIG={{config_dir}} \
        cargo run -p codegen --bin gen_helm_values > helm/factory-sim/values.yaml
    @echo "helm/factory-sim/values.yaml written"

# Deploy (or upgrade) the Helm release to the cluster
helm-deploy:
    just helm-gen
    KUBECONFIG=~/.kube/factory-sim.yaml helm upgrade --install factory-sim ./helm/factory-sim \
        -f helm/factory-sim/values.yaml \
        --set-file plantConfig={{config_dir}}/plant.json \
        --set-file deviceTypesConfig={{config_dir}}/device_types.json

# Uninstall the Helm release (leaves PVCs and ConfigMaps intact)
helm-uninstall:
    KUBECONFIG=~/.kube/factory-sim.yaml helm uninstall factory-sim

# ── Machine provisioning ──────────────────────────────────────────────────────

# Provision a device from inventory.json.  Usage: just provision workstation pi2
# Roles: workstation | k3s-server | k3s-agent | build
provision ROLE DEVICE:
    #!/bin/bash
    set -euo pipefail
    DEVICE_DATA=$(jq -r '.devices.{{DEVICE}}' {{justfile_directory()}}/deploy/inventory.json)
    HOST=$(echo "$DEVICE_DATA" | jq -r '.host')
    ENV=$(echo "$DEVICE_DATA" | jq -r '.environment')
    PROFILE=$(echo "$DEVICE_DATA" | jq -r '.credential_profile')
    CREDS=$(jq -r '.' {{justfile_directory()}}/deploy/credentials/"$ENV"/"$PROFILE".json)
    SSH_KEY=$(echo "$CREDS" | jq -r '.ssh_key')
    SSH_USER=$(echo "$CREDS" | jq -r '.ssh_user')

    echo "Provisioning {{ROLE}} on {{DEVICE}} ($SSH_USER@$HOST)..."
    ssh -A -i "$SSH_KEY" -o StrictHostKeyChecking=no "$SSH_USER@$HOST" \
        "bash -s -- --role {{ROLE}}" < {{justfile_directory()}}/deploy/provision.sh

# Provision any host by IP/hostname without an inventory entry.
# Usage: just provision-host workstation 1.2.3.4 ~/.ssh/key.pem ec2-user
provision-host ROLE HOST KEY USER="ec2-user":
    ssh -A -i {{KEY}} -o StrictHostKeyChecking=no {{USER}}@{{HOST}} \
        "bash -s -- --role {{ROLE}}" < {{justfile_directory()}}/deploy/provision.sh

# ── k3s cluster setup (run once) ──────────────────────────────────────────────

# Install k3s server on EC2 — uses Tailscale as the cluster network interface.
# Prerequisites: Tailscale must be running on EC2 (tailscale0 interface present).
k3s-server-install:
    #!/bin/bash
    set -euo pipefail
    DEVICE_DATA=$(jq -r '.devices.ec2' {{justfile_directory()}}/deploy/inventory.json)
    HOST=$(echo "$DEVICE_DATA" | jq -r '.host')
    SSH_USER=$(echo "$DEVICE_DATA" | jq -r '.ssh_user // "ec2-user"')

    echo "Provisioning k3s server on EC2 ($SSH_USER@$HOST via Tailscale SSH)..."
    tailscale ssh "$SSH_USER@$HOST" "bash -s -- --role k3s-server" \
        < {{justfile_directory()}}/deploy/provision.sh
    echo "Done. Run 'just add-device <name>' to join additional nodes."

# Add any new device to the k3s cluster, wait for it to be Ready, and label it.
# Prerequisites: entry in deploy/inventory.json + credentials file, Tailscale running on the device.
# Usage: just add-device pi2
add-device DEVICE:
    #!/bin/bash
    set -euo pipefail

    EC2_DATA=$(jq -r '.devices.ec2' {{justfile_directory()}}/deploy/inventory.json)
    EC2_HOST=$(echo "$EC2_DATA" | jq -r '.host')
    EC2_USER=$(echo "$EC2_DATA" | jq -r '.ssh_user // "ec2-user"')

    DEVICE_DATA=$(jq -r '.devices.{{DEVICE}}' {{justfile_directory()}}/deploy/inventory.json)
    DEVICE_HOST=$(echo "$DEVICE_DATA" | jq -r '.host')
    DEVICE_USER=$(echo "$DEVICE_DATA" | jq -r '.ssh_user // "pi"')

    echo "Fetching join token from EC2 (via Tailscale SSH)..."
    TOKEN=$(tailscale ssh "$EC2_USER@$EC2_HOST" 'sudo cat /var/lib/rancher/k3s/server/node-token')

    DEVICE_HOSTNAME=$(tailscale ssh "$DEVICE_USER@$DEVICE_HOST" 'hostname -s')

    echo "Installing k3s agent on {{DEVICE}} ($DEVICE_USER@$DEVICE_HOST, node: $DEVICE_HOSTNAME)..."
    tailscale ssh "$DEVICE_USER@$DEVICE_HOST" \
        "TAILSCALE_IP=\$(tailscale ip -4) && \
         curl -sfL https://get.k3s.io | \
             K3S_URL=https://$EC2_HOST:6443 \
             K3S_TOKEN=$TOKEN \
             INSTALL_K3S_EXEC=\"--node-ip=\$TAILSCALE_IP --flannel-iface=tailscale0\" \
             sh -"

    echo "Waiting for $DEVICE_HOSTNAME to be Ready..."
    export KUBECONFIG=~/.kube/factory-sim.yaml
    for i in $(seq 1 30); do
        STATUS=$(kubectl get node "$DEVICE_HOSTNAME" --no-headers 2>/dev/null | awk '{print $2}' || true)
        [ "$STATUS" = "Ready" ] && break
        echo "  ($i/30) $STATUS — retrying in 5s..."
        sleep 5
    done

    kubectl label node "$DEVICE_HOSTNAME" deploy_target={{DEVICE}} --overwrite
    echo ""
    echo "✓ {{DEVICE}} joined and labeled. Next:"
    echo "  1. Edit config/plant.json — set deploy_target for the PLC to '{{DEVICE}}'"
    echo "  2. just helm-deploy"

# Fetch kubeconfig from EC2, replace 127.0.0.1 with Tailscale hostname, save locally.
# After running this, KUBECONFIG=~/.kube/factory-sim.yaml kubectl get nodes should work.
k3s-get-kubeconfig:
    #!/bin/bash
    set -euo pipefail
    DEVICE_DATA=$(jq -r '.devices.ec2' {{justfile_directory()}}/deploy/inventory.json)
    HOST=$(echo "$DEVICE_DATA" | jq -r '.host')
    SSH_USER=$(echo "$DEVICE_DATA" | jq -r '.ssh_user // "ec2-user"')

    mkdir -p ~/.kube
    tailscale ssh "$SSH_USER@$HOST" 'sudo cat /etc/rancher/k3s/k3s.yaml' \
        | sed "s|https://127.0.0.1:6443|https://$HOST:6443|g" \
        > ~/.kube/factory-sim.yaml
    chmod 600 ~/.kube/factory-sim.yaml
    echo "Kubeconfig saved → ~/.kube/factory-sim.yaml"
    echo "Test: KUBECONFIG=~/.kube/factory-sim.yaml kubectl get nodes"

# Show node and pod status across the cluster
k3s-status:
    KUBECONFIG=~/.kube/factory-sim.yaml kubectl get nodes -o wide
    @echo ""
    KUBECONFIG=~/.kube/factory-sim.yaml kubectl get pods -o wide

# Stream logs from a named pod.  Usage: just k3s-logs backend-<hash>
k3s-logs POD:
    KUBECONFIG=~/.kube/factory-sim.yaml kubectl logs -f {{POD}}
