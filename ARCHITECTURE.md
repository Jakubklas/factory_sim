# Architecture

Industrial-twin platform. A simulator emits OPC-UA telemetry indistinguishable from real PLCs; a backend polls it; a frontend renders it. Three processes, three deployable units, two network boundaries.

---

## Top-level

```
                       ┌──────────────────────┐
                       │   config/   shared   │
                       │   plant.json         │
                       │   device_types.json  │
                       └──────────┬───────────┘
                                  │ read at startup
              ┌───────────────────┴───────────────────┐
              │                                       │
        ┌─────▼─────┐    OPC-UA      ┌────────────┐
        │ simulator │═════════╗      │  backend   │
        │  plc-001  │  :4840  ╠═════>│            │
        │ (1 PLC)   │         ║      │ connectors │
        └───────────┘         ║      │ + WS API   │
        ┌───────────┐         ║      │            │
        │ simulator │═════════╝      └─────┬──────┘
        │  plc-002  │  :4841                │ WS+HTTP
        │ (1 PLC)   │                       │  :3001
        └───────────┘                       ▼
                                     ┌──────────┐
                                     │ frontend │
                                     │ three.js │ (REST /api/plant)
                                     └──────────┘
```

**Why this shape.** OPC-UA is the boundary between "thing being measured" and "thing measuring it" — the same boundary a real PLC sits on. Each simulator process owns exactly one PLC, so cross-PLC physics wiring can't smuggle past the OPC-UA hop. Simulator and real plant are swappable; the backend never knows which one it's talking to.

---

## The four crates

```
┌─ plant_config ────────────────────────────────────┐
│  Shared schema library. No tokio, no Arc, no      │
│  state — just types and parsers.                  │
│                                                   │
│  primitives.rs   DataType, PhysicsMode, Function  │
│  schema.rs       PlantConfig, PlcConfig, Device…  │
│  resolved.rs     ResolvedPlant (merged + valid)   │
│  loader.rs       JSON → typed structs             │
│                                                   │
│  exists because: BE & simulator parse the same    │
│  JSON; shared schema keeps types in sync without  │
│  sharing runtime state.                           │
└───────────────────────────────────────────────────┘
        ▲                          ▲
        │ depends on               │ depends on
        │                          │
┌─ backend ──────────────┐  ┌─ simulator ──────────────┐
│  API + connectors.     │  │  Physics + OPC-UA hosts. │
│                        │  │                          │
│  comms/                │  │  state.rs    Simulator-  │
│   ScadaPlcConnector    │  │              State (priv)│
│   GenericConnector     │  │  tick.rs     topo-sorted │
│                        │  │              tick loop   │
│  api/ws_bridge.rs      │  │  physics_…   rhai engine │
│   /ws  /api/plant      │  │  server/     OPC-UA srv  │
│   /api/log-level       │  │              per PLC     │
│                        │  │                          │
│  plant.rs              │  │  loader.rs   PLANT_CONFIG│
│   spawn connector/PLC  │  │              + SIM_PLC_ID│
│                        │  │                          │
│  deployable alone?     │  │  deployable alone?       │
│  YES — polls any       │  │  YES — exposes OPC-UA,   │
│  reachable OPC-UA      │  │  needs nothing else      │
└────────────────────────┘  └──────────────────────────┘

┌─ codegen ─────────────────────────────────────────┐
│  Dev-time tooling only. Never runs in production. │
│                                                   │
│  src/bin/gen_helm_values.rs                       │
│    reads plant.json + platform.json +             │
│    inventory.json → writes values.yaml            │
│    run via: just helm-gen                         │
│                                                   │
│  depends on plant_config (to parse plant.json)    │
│  output is committed and consumed by Helm         │
└───────────────────────────────────────────────────┘
```

---

## Config flow

`plant_config` is shared **schema**, not shared **state**. Both binaries read the JSON, build their own `Arc<ResolvedPlant>`, and never share it across the process boundary.

```
  plant.json                     device_types.json
  (topology: PLCs + devices)     (physics + metrics)
        │                                │
        └──────────────┬─────────────────┘
                       │
              ResolvedPlant::build()
              (cross-reference + validate)
                       │
                       ▼
            ┌──────────────────────────┐
            │ ResolvedPlant            │
            │  .config                 │
            │  .devices: Vec<          │
            │     ResolvedDevice {     │
            │       config,            │
            │       type_def           │
            │     }                    │
            │  .endpoint_configs() →   │
            │     Vec<PlcEndpointCfg>  │
            └────────┬────────┬────────┘
                     │        │
              ┌──────┘        └──────┐
              ▼                      ▼
       Arc<ResolvedPlant>     Arc<ResolvedPlant>
       (in simulator)          (in backend)
       seeds SimulatorState    builds connectors
```

---

## Runtime data flow

The path of a single metric value from physics tick to browser:

```
┌─────────── simulator process ──────────────────┐
│                                                │
│  ┌──────────┐  write  ┌──────────────┐         │
│  │ physics  ├────────>│ SimulatorState│        │
│  │ (rhai)   │<────────┤   (private)  │         │
│  └──────────┘  read   └───────┬──────┘         │
│               inputs          │ snapshot       │
│                               ▼                │
│                        ┌──────────────┐        │
│                        │ OPC-UA addr  │        │
│                        │  space       │        │
│                        └───────┬──────┘        │
└────────────────────────────────┼───────────────┘
                                 │
                  · · · · · ·  TCP  · · · · · · · · ·
                                 │
┌────────────────────────────────┼───────────────┐
│ backend process                ▼               │
│                          ┌──────────────┐      │
│                          │  Scada       │      │
│                          │  Connector   │      │
│                          │   .poll()    │      │
│                          └───────┬──────┘      │
│                                  │ upsert      │
│                                  ▼             │
│                          ┌──────────────┐      │
│                          │ IngestedState│      │
│                          └───────┬──────┘      │
│                                  │ snapshot    │
│                                  ▼             │
│                          ┌──────────────┐      │
│                          │  WS bridge   │      │
│                          │  /ws  send   │      │
│                          └───────┬──────┘      │
└──────────────────────────────────┼─────────────┘
                                   │ JSON
                                   ▼
                                frontend
```

**Why two state stores** (SimulatorState + IngestedState) instead of one. Each lives on one side of the OPC-UA boundary. Simulator's is private and authoritative for simulated plants. Backend's is sourced from polling — identical to what it would see from a real PLC. Collapsing them would break the fidelity guarantee.

---

## Threading model

Each binary mixes tokio (async event loops) with std threads (blocking work).

| Process              | tokio tasks                       | std threads                |
|----------------------|-----------------------------------|----------------------------|
| simulator (per PLC)  | physics tick · OPC-UA AS updater  | one OPC-UA server          |
| backend              | WS bridge · axum HTTP             | one connector per PLC      |

State-sharing primitives:

```
  Arc<ResolvedPlant>              read-only, no lock anywhere
  Arc<RwLock<SimulatorState>>     physics writes, AS updater reads
  Arc<RwLock<IngestedState>>      connectors write, WS bridge reads
```

---

## Deployment

### Multi-node topology

The stack spans two physical machines connected by Tailscale. k3s (lightweight Kubernetes) runs on both; Tailscale is the cluster network — flannel routes all pod-to-pod traffic through the encrypted Tailscale tunnel, so no cloud firewall rules are needed beyond Tailscale itself.

```
┌─ Server node  (k3s server + worker) ──────────────────────────────────────────────────────┐
│                                                                                           │
│  ╔═ ServiceLB — binds on tailscale0 host interface ════════════════════╗                  │
│  ║  :8080 ──────────────────────────────► frontend pod :80            ║ ◄── browser       │
│  ║  :3001 ──────────────────────────────► backend pod  :3001          ║ ◄── browser (API) │
│  ╚══════════════════════════════════════════════════════════════════════╝                  │
│                                                                                           │
│  ┌─ frontend pod ───────────┐    ┌─ backend pod ───────────────────────────────────┐     │
│  │  nginx · built React SPA │    │  axum · OPC-UA connectors                       │     │
│  │  BE_URL=http://node1:3001│    │  plc-001 ──► svc/plc-001:4840  (CoreDNS)        │     │
│  └──────────────────────────┘    │  plc-002 ──► node2:4841        (Tailscale LB)   │     │
│                                  └─────────────────────────────────────────────────┘     │
│  ┌─ plc-001 pod ────────────┐    ┌─ svc: plc-001 ─────────────────────────────────┐     │
│  │  OPC-UA server   :4840   │◄───│  ClusterIP :4840  · cluster-internal only       │     │
│  └──────────────────────────┘    └────────────────────────────────────────────────-┘     │
│                                                                                           │
└───────────────────────── Tailscale VPN (WireGuard) · flannel overlay ─────────────────────┘
                            all cross-node cluster traffic is encrypted + routed here
┌─ Worker node  <node2>.your-tailnet.ts.net  (k3s agent) ───────────────────────────────────┐
│                                                                                           │
│  ┌─ plc-002 pod ────────────┐    ┌─ svc: plc-002 ─────────────────────────────────┐     │
│  │  OPC-UA server   :4841   │◄───│  LoadBalancer :4841  · binds on tailscale0      │◄────┼── backend
│  └──────────────────────────┘    │  reachable at <node2>.your-tailnet.ts.net:4841  │     │   (Tailscale)
│                                  └─────────────────────────────────────────────────┘     │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

**Why k3s, not plain Docker Compose.** Compose requires SSH-ing into each machine and running commands manually. k3s gives a single control plane: `just helm-deploy` from a laptop reaches both machines simultaneously, workloads land on the right node automatically, and crashed pods restart without intervention.

**Why Tailscale as the cluster network.** Both machines are already on a Tailscale VPN for secure remote access. Using Tailscale as the flannel interface means the cluster network is the VPN — no extra tunnels, no open ports in AWS security groups, and MagicDNS gives stable hostnames that survive IP changes.

**Why two different service types for PLCs.** A PLC on the same node as the backend needs only a ClusterIP service — CoreDNS resolves the service name, no host port consumed. A PLC on a remote node needs a LoadBalancer service so k3s ServiceLB binds the port on the host's `tailscale0` interface; the backend then connects via that node's MagicDNS hostname (set in `plant.json` as `uri`), which routes over the Tailscale tunnel.

**PLC URI convention.** `PlcConfig.uri` is `Option<String>`. Omit it when the PLC is on the same node as the backend — cluster DNS handles it. Set it explicitly only when the PLC is on a different host: another k3s node (use its MagicDNS hostname) or a real hardware PLC (use its static IP). Never hardcode `localhost` in `plant.json`; use `OPCUA_URI_OVERRIDE=opc.tcp://127.0.0.1` in a local dev `.env` instead.

---

### k3s networking: known gotchas

These were discovered during initial cluster bring-up and are documented so the next operator doesn't spend hours on them.

**CoreDNS cannot resolve `.ts.net` hostnames from inside pods.**
Pods use CoreDNS (`10.43.0.10`) as their resolver. CoreDNS doesn't forward `.ts.net` queries to the Tailscale MagicDNS server (`100.100.100.100`) because that address is only reachable from the host network, not from the pod network. The fix: add a static `NodeHosts` entry to the CoreDNS ConfigMap for any hostname that pods need to reach by Tailscale name:
```yaml
# CoreDNS ConfigMap — kube-system namespace
NodeHosts: |
  <tailscale-ip>  <hostname>.tail<tailnet>.ts.net
```
This is only needed for cross-node OPC-UA connections where the backend pod must reach a simulator on a remote node by its MagicDNS name.

**nginx `host not found in upstream` on startup.**
nginx resolves `proxy_pass` upstream hostnames at startup. If the upstream service isn't Ready yet, nginx exits. Fix: use the CoreDNS resolver IP with `resolver 10.43.0.10 valid=10s;` and a `set $upstream` variable so nginx resolves lazily per-request rather than at startup. See [frontend/docker/nginx.conf](frontend/docker/nginx.conf).

**`imagePullPolicy: Always` + `:latest` tag — Helm can't detect image changes.**
Helm compares manifests, not image digests. Pushing a new `:latest` image while the Deployment spec is unchanged causes Helm to do nothing. The `just helm-deploy` recipe runs `kubectl rollout restart deployment` after every Helm upgrade to force a pull. Keel (see below) handles automatic restarts for pushes that happen outside of `helm-deploy`.

**WebSocket over Tailscale Funnel requires a separate port.**
Tailscale Funnel proxies HTTPS traffic using HTTP/2. HTTP/2 doesn't support WebSocket upgrade (`101 Switching Protocols`). If the browser reaches the backend over Funnel on port 443, the WS handshake silently fails. Fix: expose the backend on a separate Funnel port (e.g. 8443) — the browser opens a fresh TCP connection to that port, negotiates HTTP/1.1, and the WS upgrade succeeds.

**k3s agent local load balancer (`127.0.0.1:6444`) and resource pressure.**
The k3s agent on each worker node runs a local load balancer that proxies control-plane traffic from `127.0.0.1:6444` to the server's `6443`. The agent bootstraps by posting certificate signing requests through this LB. If the k3s server node is memory- or I/O-constrained (high swap usage, iowait >50%), the server's API responses exceed the agent's default timeouts and every cert request is logged as "context deadline exceeded". The agent keeps retrying but never completes bootstrap, leaving the node `NotReady`. **Fix: ensure the server node has enough RAM** — k3s server alone consumes ~350 MB; add workload pods and you need at least 2 GB free to avoid swap. Swapping turns 1ms disk ops into 100ms+, which breaks all internal timeouts.

**`deploy_target` label lost after node re-registration.**
When a worker node leaves and rejoins the cluster (e.g. after a full agent uninstall/reinstall), its custom labels are not preserved — the node object is re-created fresh with only the default k3s labels. Any Deployments with `nodeSelector: deploy_target: <value>` will stay Pending until the label is reapplied:
```
kubectl label node <node-name> deploy_target=<value>
```
Add this step to the runbook for any worker node rebuild.

**Local-path-provisioner PVC directories are node-local.**
The default k3s storage class (`local-path`) creates directories under `/var/lib/rancher/k3s/storage/` on whichever node the pod first schedules on. The PV is then pinned to that node via node affinity. If the PVC is created while the target node is `NotReady`, the provisioner's setup job can't run, so the directory is never created even though the PV shows `Bound`. The pod then fails with `MountVolume.NewMounter initialization failed: path does not exist`. Fix: create the directory manually on the target node, or delete the PVC and let it reprovision once the node is Ready.

---

### Cluster management: K9S

K9S is a terminal UI for Kubernetes, pre-installed on the cluster server node by `deploy/provision.sh`. It gives a real-time view of nodes, pods, logs, and resource usage without writing kubectl commands.

```
k9s   # launch from any shell on the server node (uses the local kubeconfig)
```

Useful K9S shortcuts:
- `:nodes` — node health, version, resource pressure
- `:pods` — all pods across namespaces; shows READY, RESTARTS, AGE
- `l` on a pod — live log tail
- `d` on a pod — describe (events, volumes, env, conditions)
- `ctrl-k` on a pod — delete (triggers Deployment to replace it)
- `:deployments` — rollout status, desired vs available replicas
- `?` — full keybind reference

K9S reads from `~/.kube/config` by default. To use it from a developer laptop, point `KUBECONFIG` at the cluster kubeconfig (`~/.kube/factory-sim.yaml`).

---

---

### Container layout

Each binary is a leaf process: one config-volume input, one network port output.

```
┌─ pod: simulator ────────────────┐   ┌─ pod: backend ────────────────┐
│  (one per simulated PLC)        │   │                               │
│  /config  ◄─── ConfigMap (ro)   │   │  /config  ◄─── ConfigMap (ro) │
│  /data    ◄─── PVC       (rw)   │   │  /data    ◄─── PVC       (rw) │
│                                 │   │                               │
│  env: PLANT_CONFIG=/config      │   │  env: PLANT_CONFIG=/config    │
│       SIM_PLC_ID=plc-xxx        │   │       BE_HOST=0.0.0.0         │
│       SIM_TICK_MS=100           │   │       BE_PORT=3001            │
│       OPCUA_HOST=plc-xxx        │   │       BE_TICK_MS=100          │
│       PKI_DIR=/data/pki         │   │       PKI_DIR=/data/pki       │
│       SIM_HEALTH_PORT=9000      │   │                               │
│                                 │   │                               │
│  expose: <plc.port> (OPC-UA)    │   │  expose: 3001 (HTTP + WS)     │
│           9000      (health)    │   └───────────────────────────────┘
└─────────────────────────────────┘

┌─ pod: frontend ─────────────────┐
│  nginx:alpine + built SPA       │
│  env: BE_URL=http://<ec2>:3001  │
│  expose: 8080                   │
└─────────────────────────────────┘
```

`plant.json` is delivered to every pod as a Kubernetes ConfigMap. It is the single source of truth — no copies, no transformation. The ConfigMap is created at deploy time from the local `config/plant.json` via `--set-file`.

---

### CI/CD pipeline

Images are never built on the deployment target. They are built once by CI and stored in GHCR; the cluster only pulls.

```
  git push → main
        │
        ▼
  GitHub Actions  (.github/workflows/build-push.yml)
        │  cross-compiles for linux/amd64 + linux/arm64 simultaneously
        │  (no QEMU — native gcc cross-linker, vendored OpenSSL)
        │  BuildKit layer cache stored in GHCR
        │    → Rust deps only recompile when Cargo.lock changes
        ▼
  GHCR  ghcr.io/jakubklas/factory_sim/{simulator,backend,frontend}:latest
        │  multi-arch manifest — registry serves the right binary per node arch
        │
        ▼  just helm-deploy  (from developer laptop)
  Helm → k3s API server
        │  applies Deployments, Services, ConfigMap
        │  k3s scheduler places pods on correct nodes via nodeSelector
        │  each node pulls the image it needs from GHCR
        ▼
  running stack  (EC2 + worker nodes)
```

---

### Topology changes → Helm regeneration

`helm/factory-sim/values.yaml` is **generated from `plant.json`**, never edited by hand.

```
  config/plant.json       edit: add PLCs, change ports, set deploy_target
        │
        ▼
  just helm-gen           runs codegen/src/bin/gen_helm_values.rs
        │                 reads plant.json + platform.json + inventory.json
        │                 emits one PLC entry per simulated PLC with
        │                 correct serviceType (ClusterIP vs LoadBalancer)
        ▼
  helm/factory-sim/values.yaml   commit this
        │
        ▼
  just helm-deploy        helm upgrade --install + --set-file plantConfig=...
                          cluster converges to new topology
```

---

### Adding a new device

Adding a new physical machine (another cloud VM, an edge device, bare metal) to the cluster is a two-command operation after registering it in `deploy/inventory.json`:

```
just add-device pi2      # joins pi2 to the k3s cluster and labels it
# edit plant.json → set deploy_target for a PLC to "pi2"
just helm-deploy         # scheduler places the pod on pi2
```

`add-device` fetches the cluster join token from EC2, installs the k3s agent on the new device over SSH, waits for the node to be Ready, and applies the `deploy_target` label — no manual kubectl needed.

---

### Key commands

| Command | What it does |
|---|---|
| `just be` | Run backend locally (cargo) |
| `just sim plc-001` | Run one simulator locally (cargo) |
| `just helm-gen` | Regenerate `values.yaml` from `plant.json` |
| `just helm-deploy` | Deploy / upgrade the full stack on k3s |
| `just add-device DEVICE` | Join a new machine to the cluster |
| `just k3s-status` | Show node + pod status across the cluster |
| `just k3s-logs POD` | Stream logs from a named pod |

---

### Env-var surface

| Var                  | Used by   | Purpose                                                              |
|----------------------|-----------|----------------------------------------------------------------------|
| `PLANT_CONFIG`       | both      | Path to directory holding `plant.json` + `device_types.json`         |
| `SIM_PLC_ID`         | simulator | Which PLC this process owns (required)                               |
| `SIM_TICK_MS`        | simulator | Physics tick cadence (default 100)                                   |
| `SIM_HEALTH_PORT`    | simulator | HTTP health endpoint port (default 9000)                             |
| `OPCUA_HOST`         | simulator | Hostname advertised in OPC-UA discovery URLs                         |
| `PKI_DIR`            | both      | Where the OPC-UA crate writes its auto-generated keypair             |
| `BE_HOST/PORT/TICK_MS` | backend | WS+HTTP bind address + tick rate                                     |
| `OPCUA_URI_OVERRIDE` | backend   | Rewrite the host portion of every PLC URL — dev-only escape hatch    |

---

## API surface

### Backend (`BE_PORT`, default 3001)

| Endpoint                | Method | Returns                                  |
|-------------------------|--------|------------------------------------------|
| `/ws`                   | WS     | full `IngestedState` every tick (JSON)   |
| `/api/plant`            | GET    | `PlantConfig` — static topology         |
| `/api/log-level?set=…`  | GET    | reload tracing filter at runtime         |
| `/health`               | GET    | `{"status":"ok"}` — liveness probe      |

### Simulator (`SIM_HEALTH_PORT`, default 9000)

| Endpoint   | Method | Returns                             |
|------------|--------|-------------------------------------|
| `/health`  | GET    | `{"status":"ok"}` — liveness probe |

---

## Evaluation

Things worth fixing:

1. **Connectors use `std::thread`, not tokio tasks.** Historical: `opcua-rs 0.12` has a sync client API. One OS thread per PLC is fine at this scale; reconsider when scaling to dozens of PLCs or when an async opcua client crate is viable.

2. **Config is file-based, not API-driven.** `plant.json` is baked into a ConfigMap at deploy time. The intended path: backend seeds its DB from `plant.json` on first boot, then exposes `GET/PUT /api/config` so the frontend can edit the plant topology live without redeploying. The ConfigMap then disappears — pods fetch config from the backend API.

Things that are fine and worth not re-debating:

- Full `IngestedState` cloned per WS frame — small, JSON-serialisable, no measurable cost.
- `RwLock` instead of `watch`/channels for state — straightforward, no contention at tick-ms cadence.

Recently fixed:

- **Simulator `/health` endpoint** — each simulator now exposes `GET /health` on port 9000 (axum, separate from the OPC-UA port); k3s liveness probe wired in the Helm template.
- **PVC for PKI data** — both simulator and backend pods use a `PersistentVolumeClaim` for `/data` instead of `emptyDir`; OPC-UA keypairs survive pod restarts, eliminating cert-churn backoff storms.
- **OPC-UA reconnect loop** — `opcua 0.12` shares an async runtime across `Client` instances; dropping the old `PlcConnection` while creating a new one closed the new session's message sender, causing every reconnect attempt to fail permanently. Fixed by calling `drop(conn)` before `connect_with_backoff()` in `GenericConnector`. The underlying cause (shared runtime in `opcua 0.12`) is tracked as a must-fix; see `PLAN.md` weaknesses.
- **Keel auto-deploy** — Keel runs in `kube-system` and polls GHCR every 5 minutes; annotated Deployments are restarted automatically when a new `:latest` image is pushed. Eliminates manual `helm-deploy` for image-only changes.
