# Architecture

Industrial-twin platform. A simulator emits OPC-UA telemetry indistinguishable from real PLCs; a backend polls it; a frontend renders it. Three processes, three deployable units, two network boundaries.

---

## Top-level

```
                       ┌──────────────────────┐
                       │   config/   shared   │
                       │   plant.json         │
                       │   device_types.json  │
                       │   BE_HOST/PORT/TICK  │
                       └──────────┬───────────┘
                                  │ read at startup
              ┌───────────────────┴───────────────────┐
              │                                       │
        ┌─────▼─────┐    OPC-UA      ┌────────────┐   │
        │ simulator │═════════╗      │  backend   │   │
        │  plc_001  │  :4840  ╠═════>│            │   │
        │ (1 PLC)   │         ║      │ connectors │   │
        └───────────┘         ║      │ + WS API   │   │
        ┌───────────┐         ║      │            │   │
        │ simulator │═════════╝      └─────┬──────┘   │
        │  plc_002  │  :4841                │ WS+HTTP │
        │ (1 PLC)   │                       │  :3001  │
        └───────────┘                       ▼         │
                                     ┌──────────┐     │
                                     │ frontend │◄────┘
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
        │                          │
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
│  Dev-time tooling only. Never runs in production.  │
│                                                    │
│  src/bin/gen_compose.rs                            │
│    reads plant.json → writes docker-compose.yml    │
│    run via: just compose-gen                       │
│                                                    │
│  depends on plant_config (to parse plant.json)     │
│  output is committed and consumed by Docker        │
└────────────────────────────────────────────────────┘
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

### Container layout

Each binary is a leaf process: one config-volume input, one network port output, no shared filesystem. The stack is `N + 2` containers — N simulators (one per `simulated: true` PLC in `plant.json`) + backend + frontend.

```
┌─ container: simulator ──────┐   ┌─ container: backend ──────┐
│  (one per simulated PLC)    │   │                           │
│  /config  ◄─── volume (ro)  │   │  /config  ◄─── volume(ro) │
│  /data    ◄─── volume (rw)  │   │  /data    ◄─── volume(rw) │
│                             │   │                           │
│  env: PLANT_CONFIG=/config  │   │  env: PLANT_CONFIG=/config│
│       SIM_PLC_ID=plc_xxx    │   │       BE_HOST=0.0.0.0     │
│       SIM_TICK_MS=100       │   │       BE_PORT=3001        │
│       OPCUA_HOST=plc_xxx    │   │       BE_TICK_MS=100      │
│       PKI_DIR=/data/pki     │   │       PKI_DIR=/data/pki   │
│                             │   │                           │
│  expose: <plc.port> (OPC-UA)│   │  expose: 3001 (HTTP + WS) │
└─────────────────────────────┘   └───────────────────────────┘

┌─ container: frontend ───────┐
│  nginx:alpine + built SPA   │
│  env: BE_URL=http://<ip>:3001  (written by bootstrap or .env)
│  expose: 8080               │
└─────────────────────────────┘
```

In dev: `just sim-all` + `just be` + `npm run dev`. Set `OPCUA_URI_OVERRIDE=opc.tcp://127.0.0.1` in `.env` so the backend reaches simulators on loopback. In prod: Docker's service-name DNS resolves `opc.tcp://{plc_id}` natively — no override needed.

**PLC URI convention.** `PlcConfig.uri` is `Option<String>`. When absent (the default), the backend derives `opc.tcp://{plc_id}` — the Docker/k8s service-name DNS form. Set it explicitly only when a PLC lives on a static IP (e.g. a real hardware PLC at `opc.tcp://192.168.1.10`). Never hardcode `localhost` in `plant.json`; use `OPCUA_URI_OVERRIDE` in a local dev `.env` instead.

---

### Topology changes → compose regeneration

`docker-compose.yml` is **generated from `plant.json`**, never edited by hand. The `codegen` crate contains the generator.

```
  config/plant.json       edit: add PLCs, change ports, flip simulated: true
        │
        ▼
  just compose-gen        runs codegen/src/bin/gen_compose.rs
        │                 one service per simulated PLC, image refs point to GHCR
        ▼
  docker-compose.yml      commit this — consumed by Docker on every host
```

Run `just compose-gen` whenever the PLC topology changes, then commit the result. The service-name convention (`opc.tcp://{plc_id}`) works unchanged under k8s.

---

### CI/CD pipeline

Images are never built on the deployment target. They are built once by CI and stored in GHCR; hosts only pull.

```
  git push → main
        │
        ▼
  GitHub Actions  (.github/workflows/build-push.yml)
        │  builds simulator, backend, frontend
        │  BuildKit layer cache stored in GHCR
        │    → Rust deps only recompile when Cargo.lock changes
        │    → warm build ~1 min,  cold build ~15 min
        ▼
  GHCR  ghcr.io/jakubklas/factory_sim/{simulator,backend,frontend}:latest
        │
        ▼  just redeploy <host>
  deployment host
        │  git pull          (picks up new docker-compose.yml if topology changed)
        │  docker compose pull
        │  docker compose up -d
        ▼
  running stack
```

**Key commands:**

| Command | What it does |
|---|---|
| `just compose-gen` | Regenerate `docker-compose.yml` from `plant.json` |
| `just push` | Build + push images locally (escape hatch; needs `write:packages` scope) |
| `just deploy <host>` | Bootstrap a fresh Linux machine: install Docker, clone repo, pull images, start stack |
| `just redeploy <host>` | On a running host: `git pull` + `docker compose pull` + restart |
| `just remote-logs <host>` | Stream live logs from a deployed host |

---

**Env-var surface:**

| Var                  | Used by   | Purpose                                                              |
|----------------------|-----------|----------------------------------------------------------------------|
| `PLANT_CONFIG`       | both      | Path to directory holding `plant.json` + `device_types.json`         |
| `SIM_PLC_ID`         | simulator | Which PLC this process owns (required)                               |
| `SIM_TICK_MS`        | simulator | Physics tick cadence (default 100)                                   |
| `OPCUA_HOST`         | simulator | Hostname advertised in OPC-UA discovery URLs (default: `HOSTNAME`)   |
| `PKI_DIR`            | both      | Where the OPC-UA crate writes its auto-generated keypair (default `./pki`) |
| `BE_HOST/PORT/TICK_MS` | backend | WS+HTTP bind address + tick rate                                     |
| `OPCUA_URI_OVERRIDE` | backend   | Rewrite the host portion of every PLC URL — dev-only escape hatch    |

---

## Backend API surface

| Endpoint                | Method | Returns                                  |
|-------------------------|--------|------------------------------------------|
| `/ws`                   | WS     | full `IngestedState` every tick (JSON)   |
| `/api/plant`            | GET    | `PlantConfig` — static topology         |
| `/api/log-level?set=…`  | GET    | reload tracing filter at runtime         |
| `/health`               | GET    | `{"status":"ok"}` — Docker healthcheck  |

---

## Evaluation

Things worth fixing:

1. **Connectors use `std::thread`, not tokio tasks.** Historical: `opcua-rs 0.12` has a sync client API. One OS thread per PLC is fine at this scale; reconsider when scaling to dozens of PLCs or when an async opcua client crate is viable.

Things that are fine and worth not re-debating:

- Full `IngestedState` cloned per WS frame — small, JSON-serialisable, no measurable cost.
- `RwLock` instead of `watch`/channels for state — straightforward, no contention at tick-ms cadence.