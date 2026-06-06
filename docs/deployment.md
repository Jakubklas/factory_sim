# Deployment & operations

The platform runs on **k3s** across a fixed set of machines (cloud + edge) joined by
**Tailscale**, which doubles as the cluster network. Tasks are driven by the
[`Justfile`](../Justfile); machine inventory lives in [`deploy/`](../deploy/).

```
  browser ── HTTPS ─► Tailscale Funnel ─► frontend  (:443 → 8080)
                                       └► backend WS (:8443 → 3001)
                                          │
   ┌─ k3s · Tailscale-as-network ─────────┴──────────────────────────────────┐
   │  backend ── browses/reads/writes ──► sim PLC pods (ClusterIP plc-*:4840) │
   │  frontend         Postgres          spread across nodes by deploy_node   │
   └──────────────────────────────────────────────────────────────────────────┘
```

- **Tailscale is the cluster network.** k3s flannel rides the encrypted tunnel
  (`--flannel-iface=tailscale0`), so no firewall ports are opened between nodes.
- **Tailscale Funnel** exposes the frontend and backend to the public internet. The backend
  gets its **own** Funnel port (8443) because Funnel proxies HTTP/2, which can't upgrade the
  WebSocket on the frontend's port.
- **The backend manages sim pods** via `kubectl` using a tightly-scoped ServiceAccount
  (Role/RoleBinding for Deployments/Services/PVCs in its namespace —
  [`helm/factory-sim/templates/rbac.yaml`](../helm/factory-sim/templates/rbac.yaml)).

---

## Build & release

CI ([`.github/workflows/build-push.yml`](../.github/workflows/build-push.yml)) builds
**multi-arch (amd64 + arm64)** images for backend, frontend, and simulator on every push to
`main`, pushing to GHCR (`ghcr.io/jakubklas/factory_sim/*`) tagged `latest` and `main-<sha>`.

**Keel** (`just keel-install`) polls GHCR every ~5 min and rolls the Deployments when a new
image digest lands — no manual `kubectl rollout`.

---

## Deploying the platform

`just helm-deploy` installs/upgrades the Helm release (backend, frontend, RBAC, configmaps),
runs PVC migration for any relocated PLC, restarts deployments, and restores Funnel.

> The **platform** is deployed by Helm. The **plant itself** (device types, PLCs, wires) is
> built live through the API — never via Helm. Adding a simulated PLC row is what causes the
> backend to create its pod.

Common recipes:

| Recipe | Does |
|---|---|
| `just be` / `just sim <id>` / `just sim-all` | Run backend / one sim / all sims locally. |
| `just helm-gen` | Generate `values.yaml` from `plant.json` + `platform.json` + `inventory.json`. |
| `just helm-deploy` | Generate values, upgrade the release, migrate PVCs, restart, set up Funnel. |
| `just keel-install` / `keel-uninstall` | Enable / disable image auto-updates. |
| `just k3s-status` / `k3s-logs <pod>` | Cluster status / pod logs. |
| `just funnel-setup` | Re-establish Tailscale Funnel (run after reboots). |

---

## Cluster setup (one-time) & nodes

| Recipe | Does |
|---|---|
| `just k3s-server-install` | Install the k3s server on EC2 over Tailscale. |
| `just k3s-get-kubeconfig` | Fetch kubeconfig, rewrite the API host to the Tailscale name. |
| `just add-device <name>` | Join a node from `inventory.json`, wait for Ready, label `deploy_target`. |
| `just provision <role> <device>` | Run [`deploy/provision.sh`](../deploy/provision.sh) on an inventory device. |
| `just k3s-tune-gc` | Tune kubelet image GC (60%/40%) so small edge disks don't fill. |

Inventory and credentials: [`deploy/inventory.json`](../deploy/inventory.json),
[`deploy/platform.json`](../deploy/platform.json), `deploy/credentials/`.

---

## Operational gotchas (hard-won)

- Funnel proxies HTTP/2 → can't upgrade WebSockets, so the backend has its own Funnel port (8443).
- nginx must resolve upstreams **lazily** via the CoreDNS IP, or it exits when a service isn't Ready yet.
- The k3s server node needs **≥ 2 GB free RAM**; swap breaks control-plane timeouts → nodes go `NotReady`.
- A re-joined node loses custom labels — reapply `deploy_target` or its sim pods stay `Pending`.
- `local-path` PVCs are node-pinned; one bound while its node is `NotReady` never gets its directory.
  (`just _migrate-pvcs`, run inside `helm-deploy`, deletes PVCs stranded on the wrong node.)
- Image GC is tuned (60%/40%) so accumulated layers don't fill small edge disks.
