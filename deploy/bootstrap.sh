#!/usr/bin/env bash
# Bootstrap a fresh Linux host for factory_sim.
# Idempotent — safe to re-run on an already-configured machine.
set -euo pipefail

REPO_URL="https://github.com/Jakubklas/factory_sim.git"
APP_DIR="$HOME/factory_sim"
COMPOSE_VERSION="v2.32.4"
COMPOSE_BIN="/usr/local/lib/docker/cli-plugins/docker-compose"

log() { echo "▶ $*"; }

# ── Swap ────────────────────────────────────────────────────────────────────
# Rust compilation needs ~2 GB. Add a 4 GB swapfile if total RAM is under 2 GB
# and no swap is already active.
total_mem_mb=$(awk '/MemTotal/ { printf "%d", $2/1024 }' /proc/meminfo)
swap_total=$(awk '/SwapTotal/ { printf "%d", $2/1024 }' /proc/meminfo)

if [ "$total_mem_mb" -lt 2048 ] && [ "$swap_total" -eq 0 ] && [ ! -f /swapfile ]; then
    log "RAM=${total_mem_mb}MB — adding 4 GB swap"
    sudo fallocate -l 4G /swapfile
    sudo chmod 600 /swapfile
    sudo mkswap /swapfile
    sudo swapon /swapfile
    grep -q '/swapfile' /etc/fstab || echo '/swapfile swap swap defaults 0 0' | sudo tee -a /etc/fstab
elif [ -f /swapfile ] && [ "$swap_total" -eq 0 ]; then
    log "Existing swapfile found — re-enabling"
    sudo swapon /swapfile 2>/dev/null || true
else
    log "RAM=${total_mem_mb}MB, swap=${swap_total}MB — no swap needed"
fi

# ── Package manager detection ────────────────────────────────────────────────
if command -v dnf &>/dev/null; then
    PKG="sudo dnf install -y"
elif command -v apt-get &>/dev/null; then
    sudo apt-get update -qq
    PKG="sudo apt-get install -y"
else
    echo "Unsupported package manager — install Docker and git manually." >&2
    exit 1
fi

# ── Git ──────────────────────────────────────────────────────────────────────
if ! command -v git &>/dev/null; then
    log "Installing git"
    $PKG git
fi
log "git $(git --version)"

# ── Docker ───────────────────────────────────────────────────────────────────
if ! command -v docker &>/dev/null; then
    os_id=$(. /etc/os-release && echo "$ID")
    if [ "$os_id" = "amzn" ]; then
        log "Installing Docker (Amazon Linux)"
        sudo dnf install -y docker
    else
        log "Installing Docker via convenience script"
        curl -fsSL https://get.docker.com | sudo sh
    fi
fi
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER" 2>/dev/null || true
log "docker $(docker --version)"

# ── Docker Compose v2 ────────────────────────────────────────────────────────
if ! sudo docker compose version &>/dev/null; then
    log "Installing Docker Compose $COMPOSE_VERSION"
    ARCH=$(uname -m)
    [ "$ARCH" = "aarch64" ] && ARCH="aarch64" || ARCH="x86_64"
    sudo mkdir -p "$(dirname "$COMPOSE_BIN")"
    sudo curl -fsSL \
        "https://github.com/docker/compose/releases/download/${COMPOSE_VERSION}/docker-compose-linux-${ARCH}" \
        -o "$COMPOSE_BIN"
    sudo chmod +x "$COMPOSE_BIN"
fi
log "compose $(sudo docker compose version)"

# ── Repo ─────────────────────────────────────────────────────────────────────
if [ -d "$APP_DIR/.git" ]; then
    log "Updating existing repo"
    git -C "$APP_DIR" pull --ff-only
else
    log "Cloning repo"
    git clone "$REPO_URL" "$APP_DIR"
fi

# ── .env for compose (BE_URL uses the instance's public IP) ─────────────────
cd "$APP_DIR"
if [ ! -f .env ]; then
    PUBLIC_IP=$(curl -sf --max-time 2 http://169.254.169.254/latest/meta-data/public-ipv4 2>/dev/null \
        || hostname -I | awk '{print $1}')
    log "Public IP: ${PUBLIC_IP}"
    echo "BE_URL=http://${PUBLIC_IP}:3001" > .env
    log "Wrote .env"
fi

# ── Pull + start ──────────────────────────────────────────────────────────────
log "Pulling images from registry"
sudo docker compose pull

log "Starting stack"
sudo docker compose up -d

log "Stack is up:"
sudo docker compose ps
