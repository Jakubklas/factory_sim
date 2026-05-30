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

**Why two different service types for PLCs.** plc-001 sits on the same node as the backend — ClusterIP is sufficient, CoreDNS resolves `plc-001` to it, no host port wasted. plc-002 is on a separate machine; its `uri` in `plant.json` points to the worker node's MagicDNS hostname, so a LoadBalancer is needed to bind port 4841 on that node's host interface (including `tailscale0`), making it reachable at that address.

**PLC URI convention.** `PlcConfig.uri` is `Option<String>`. Omit it when the PLC is on the same node as the backend — cluster DNS handles it. Set it explicitly only when the PLC is on a different host: another k3s node (use its MagicDNS hostname) or a real hardware PLC (use its static IP). Never hardcode `localhost` in `plant.json`; use `OPCUA_URI_OVERRIDE=opc.tcp://127.0.0.1` in a local dev `.env` instead.

---

### Container layout

Each binary is a leaf process: one config-volume input, one network port output.

```
┌─ pod: simulator ────────────────┐   ┌─ pod: backend ────────────────┐
│  (one per simulated PLC)        │   │                               │
│  /config  ◄─── ConfigMap (ro)   │   │  /config  ◄─── ConfigMap (ro) │
│  /data    ◄─── emptyDir  (rw)   │   │  /data    ◄─── emptyDir  (rw) │
│                                 │   │                               │
│  env: PLANT_CONFIG=/config      │   │  env: PLANT_CONFIG=/config    │
│       SIM_PLC_ID=plc-xxx        │   │       BE_HOST=0.0.0.0         │
│       SIM_TICK_MS=100           │   │       BE_PORT=3001            │
│       OPCUA_HOST=plc-xxx        │   │       BE_TICK_MS=100          │
│       PKI_DIR=/data/pki         │   │       PKI_DIR=/data/pki       │
│                                 │   │                               │
│  expose: <plc.port> (OPC-UA)    │   │  expose: 3001 (HTTP + WS)     │
└─────────────────────────────────┘   └───────────────────────────────┘

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
| `OPCUA_HOST`         | simulator | Hostname advertised in OPC-UA discovery URLs                         |
| `PKI_DIR`            | both      | Where the OPC-UA crate writes its auto-generated keypair             |
| `BE_HOST/PORT/TICK_MS` | backend | WS+HTTP bind address + tick rate                                     |
| `OPCUA_URI_OVERRIDE` | backend   | Rewrite the host portion of every PLC URL — dev-only escape hatch    |

---

## Backend API surface

| Endpoint                | Method | Returns                                  |
|-------------------------|--------|------------------------------------------|
| `/ws`                   | WS     | full `IngestedState` every tick (JSON)   |
| `/api/plant`            | GET    | `PlantConfig` — static topology         |
| `/api/log-level?set=…`  | GET    | reload tracing filter at runtime         |
| `/health`               | GET    | `{"status":"ok"}` — liveness probe      |

---

## Evaluation

Things worth fixing:

1. **Connectors use `std::thread`, not tokio tasks.** Historical: `opcua-rs 0.12` has a sync client API. One OS thread per PLC is fine at this scale; reconsider when scaling to dozens of PLCs or when an async opcua client crate is viable.

2. **Config is file-based, not API-driven.** `plant.json` is baked into a ConfigMap at deploy time. The intended path: backend seeds its DB from `plant.json` on first boot, then exposes `GET/PUT /api/config` so the frontend can edit the plant topology live without redeploying. The ConfigMap then disappears — pods fetch config from the backend API.

Things that are fine and worth not re-debating:

- Full `IngestedState` cloned per WS frame — small, JSON-serialisable, no measurable cost.
- `RwLock` instead of `watch`/channels for state — straightforward, no contention at tick-ms cadence.
