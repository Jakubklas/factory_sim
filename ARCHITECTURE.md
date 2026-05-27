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

## The three crates

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
│  exists because: BE & simulator must agree on     │
│  what a plant looks like, without sharing state.  │
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

Each binary is a leaf process with one config-volume input and one network port output. No shared filesystem or runtime state. One simulator container per simulated PLC; the topology (which PLCs exist) lives entirely in `plant.json`, so orchestration is `N + 1` containers derived mechanically from config.

```
┌─ container: simulator ──────┐   ┌─ container: backend ──────┐
│  (one per simulated PLC)    │   │                           │
│                             │   │  /config  ◄─── volume     │
│  /config  ◄─── volume       │   │  /data    ◄─── volume(rw) │
│  /data    ◄─── volume(rw)   │   │                           │
│                             │   │  env: PLANT_CONFIG=/config│
│  env: PLANT_CONFIG=/config  │   │       BE_HOST=0.0.0.0     │
│       SIM_PLC_ID=plc_xxx    │   │       BE_PORT=3001        │
│       SIM_TICK_MS=100       │   │       BE_TICK_MS=100      │
│       OPCUA_HOST=plc_xxx    │   │       PKI_DIR=/data/pki   │
│       PKI_DIR=/data/pki     │   │       OPCUA_URI_OVERRIDE= │
│                             │   │           (unset)         │
│  bind: 0.0.0.0              │   │                           │
│  expose: <plc.port>         │   │  expose: 3001             │
│  (OPC-UA, single port)      │   │  (HTTP + WS)              │
└─────────────────────────────┘   └───────────────────────────┘
```

In dev: `just sim-all` + `just be`. `.env` sets `OPCUA_HOST=127.0.0.1` and `OPCUA_URI_OVERRIDE=opc.tcp://127.0.0.1` so the host machine reaches simulators on loopback. In prod: `N + 1` containers (one simulator per `simulated: true` PLC in `plant.json`, plus the backend), `config/` volume-mounted into each. Docker's service-name DNS resolves `opc.tcp://{plc_id}:port` natively — no override needed.

**PLC URI convention.** `PlcConfig.uri` is `Option<String>`. When absent (the default), the backend derives `opc.tcp://{plc_id}` — which is the Docker/k8s service-name DNS form. Set it explicitly in `plant.json` only when the PLC lives on a static IP or a non-standard hostname (e.g. a real hardware PLC at `opc.tcp://192.168.1.10`). Never set it to `localhost` in `plant.json`; use `OPCUA_URI_OVERRIDE` for that in the local dev `.env` file.

**Containerization.** `just compose-gen` regenerates `docker-compose.yml` from `plant.json`. Each simulated PLC becomes its own service named after its `plc_id`. Rebuild when the PLC topology changes. The same service-name convention works under k8s with no changes to `plant.json`.

**Registry & CI/CD.** Images are published to `ghcr.io/jakubklas/factory_sim/{simulator,backend,frontend}:latest`. Every push to `main` triggers a GitHub Actions workflow (`.github/workflows/build-push.yml`) that builds and pushes all three images using BuildKit layer caching stored in GHCR — so Rust dependency layers survive between runs and only recompile when `Cargo.lock` changes. Deployment targets pull pre-built images; they never compile from source.

- `just push` — build + push locally (manual escape hatch; requires `gh auth refresh -h github.com -s write:packages`)
- `just deploy <host>` — bootstrap a fresh Linux machine: installs Docker, clones repo, pulls images, starts stack
- `just redeploy <host>` — on an already-running host: `git pull` + `docker compose pull` + restart

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