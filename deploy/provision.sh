#!/usr/bin/env bash
# provision.sh --role <role>
#
# Roles:
#   workstation  — git, curl, jq, Tailscale, just, Rust, Node 22, Claude Code
#   k3s-server   — git, curl, jq, Tailscale, just, k3s server (labels node deploy_target=ec2)
#   k3s-agent    — git, curl, jq, Tailscale, k3s agent  (requires K3S_URL + K3S_TOKEN env)
#   build        — git, curl, jq, Tailscale, just, Rust, Docker, arm64 cross-compile toolchain
#
# Run locally:
#   bash deploy/provision.sh --role workstation
#
# Run remotely (via just):
#   just provision workstation <device>          # device must be in deploy/inventory.json
#   just provision-host workstation 1.2.3.4 ~/.ssh/key.pem ec2-user
#
# Idempotent — safe to re-run; already-installed tools are skipped.

set -euo pipefail

ROLE=""
while [[ $# -gt 0 ]]; do
  case $1 in
    --role) ROLE="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$ROLE" ]]; then
  echo "Usage: $0 --role <workstation|k3s-server|k3s-agent|build>" >&2
  exit 1
fi

# ── OS detection ──────────────────────────────────────────────────────────────

if [[ -f /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  OS_ID="${ID:-unknown}"
  OS_LIKE="${ID_LIKE:-}"
else
  OS_ID="unknown"; OS_LIKE=""
fi

is_amazon() { [[ "$OS_ID" == "amzn" ]]; }
is_debian()  { [[ "$OS_ID" == "debian" || "$OS_ID" == "ubuntu" || "$OS_LIKE" == *"debian"* ]]; }

pkg_update() {
  is_debian && sudo apt-get update -qq || true
}

pkg_install() {
  if is_amazon; then sudo yum install -y "$@"
  elif is_debian; then sudo apt-get install -y "$@"
  else echo "Unsupported OS: $OS_ID" >&2; exit 1
  fi
}

# ── Logging ───────────────────────────────────────────────────────────────────

log() { echo ""; echo "━━━  $*  ━━━"; }

# ── Components ────────────────────────────────────────────────────────────────

install_base() {
  log "Base packages"
  pkg_update
  pkg_install git curl jq
}

install_tailscale() {
  if command -v tailscale &>/dev/null; then echo "  tailscale already installed"; return; fi
  log "Tailscale"
  curl -fsSL https://tailscale.com/install.sh | sh
  sudo systemctl enable --now tailscaled
  echo "  → Next: sudo tailscale up --hostname=<name>"
}

install_just() {
  if command -v just &>/dev/null; then echo "  just already installed"; return; fi
  log "just (task runner)"
  curl -fsSL https://just.systems/install.sh | sudo bash -s -- --to /usr/local/bin
}

install_rust() {
  if command -v cargo &>/dev/null; then echo "  Rust already installed"; return; fi
  log "Rust (rustup)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
  echo 'source "$HOME/.cargo/env"' >> ~/.bashrc
}

install_node() {
  export NVM_DIR="$HOME/.nvm"
  if [[ -s "$NVM_DIR/nvm.sh" ]]; then
    echo "  nvm already installed"
    # shellcheck disable=SC1091
    source "$NVM_DIR/nvm.sh"
    return
  fi
  log "Node.js 22 (via nvm)"
  curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
  # shellcheck disable=SC1091
  source "$NVM_DIR/nvm.sh"
  nvm install 22 --silent
  nvm alias default 22
}

install_claude() {
  export NVM_DIR="$HOME/.nvm"
  [[ -s "$NVM_DIR/nvm.sh" ]] && source "$NVM_DIR/nvm.sh"
  if command -v claude &>/dev/null; then echo "  Claude Code already installed"; return; fi
  log "Claude Code"
  npm install -g @anthropic-ai/claude-code
}

install_docker() {
  if command -v docker &>/dev/null; then echo "  Docker already installed"; return; fi
  log "Docker"
  if is_amazon; then
    sudo yum install -y docker
  elif is_debian; then
    curl -fsSL https://get.docker.com | sh
  fi
  sudo systemctl enable --now docker
  sudo usermod -aG docker "$USER"
  echo "  → Re-login or run: newgrp docker"
}

install_cross_toolchain() {
  log "Cross-compilation toolchain (amd64 ↔ arm64)"
  if is_debian; then
    sudo apt-get install -y \
      gcc-aarch64-linux-gnu \
      libc6-dev-arm64-cross \
      binutils-aarch64-linux-gnu \
      gcc-x86-64-linux-gnu \
      libc6-dev-amd64-cross
  elif is_amazon; then
    # Amazon Linux 2023 lacks the full cross-libc packages; use cross (Docker-based)
    sudo yum install -y binutils-aarch64-linux-gnu gcc-aarch64-linux-gnu || true
    if command -v cargo &>/dev/null; then
      # shellcheck disable=SC1091
      source "$HOME/.cargo/env"
      cargo install cross --quiet
      echo "  → Use: cross build --target aarch64-unknown-linux-gnu"
    fi
  fi
}

install_k3s_server() {
  if command -v k3s &>/dev/null; then echo "  k3s already installed"; return; fi
  log "k3s server"
  if ! command -v tailscale &>/dev/null; then
    echo "  ERROR: Tailscale must be installed and connected first" >&2; exit 1
  fi
  TAILSCALE_IP=$(tailscale ip -4)
  curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="
    --node-ip=$TAILSCALE_IP
    --advertise-address=$TAILSCALE_IP
    --bind-address=$TAILSCALE_IP
    --flannel-iface=tailscale0
    --disable=traefik
  " sh -
  NODE=$(sudo k3s kubectl get nodes -o jsonpath='{.items[0].metadata.name}')
  sudo k3s kubectl label node "$NODE" deploy_target=ec2 --overwrite
  echo "  → Join token: sudo cat /var/lib/rancher/k3s/server/node-token"
}

install_k3s_agent() {
  if command -v k3s &>/dev/null; then echo "  k3s already installed"; return; fi
  log "k3s agent"
  if [[ -z "${K3S_URL:-}" || -z "${K3S_TOKEN:-}" ]]; then
    echo "  ERROR: K3S_URL and K3S_TOKEN must be set" >&2
    echo "  Tip: use 'just add-device DEVICE' which sets these automatically" >&2
    exit 1
  fi
  TAILSCALE_IP=$(tailscale ip -4)
  curl -sfL https://get.k3s.io | \
    K3S_URL="$K3S_URL" \
    K3S_TOKEN="$K3S_TOKEN" \
    INSTALL_K3S_EXEC="--node-ip=$TAILSCALE_IP --flannel-iface=tailscale0" \
    sh -
}

# ── Roles ─────────────────────────────────────────────────────────────────────

case "$ROLE" in
  workstation)
    install_base
    install_tailscale
    install_just
    install_rust
    install_node
    install_claude
    ;;
  k3s-server)
    install_base
    install_tailscale
    install_just
    install_k3s_server
    ;;
  k3s-agent)
    install_base
    install_tailscale
    install_k3s_agent
    ;;
  build)
    install_base
    install_tailscale
    install_just
    install_rust
    install_docker
    install_cross_toolchain
    ;;
  *)
    echo "Unknown role: $ROLE" >&2
    echo "Roles: workstation | k3s-server | k3s-agent | build" >&2
    exit 1
    ;;
esac

# ── Summary ───────────────────────────────────────────────────────────────────

log "Done: $ROLE on $(hostname)"
echo ""
_v() { command -v "$1" &>/dev/null && echo "  $1: $($2)" || true; }
_v git        "git --version"
_v just       "just --version"
_v cargo      "cargo --version"
_v node       "node --version"
_v claude     "claude --version"
_v docker     "docker --version"
_v k3s        "k3s --version | head -1"
_v tailscale  "tailscale --version | head -1"
echo ""
