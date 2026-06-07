# factory-sim — a runtime-built industrial-plant digital twin

Author *device types* (a physics model + I/O contract), assemble them into *PLCs*
(simulated or real), wire any device's output to any other device's input, and the
**backend discovers and runs the whole plant over OPC-UA** — reading telemetry,
routing wired values, writing setpoints, and spinning up simulated PLCs on demand.

The plant lives in **Postgres**, not in files. Changing it is an API call, never a
redeploy. Simulated and real PLCs are indistinguishable to the backend: both are
**browsed** to learn their address space, both are read and written the same way.

```
   author device types ─► assemble PLCs ─► wire devices ──► (all in Postgres)
                                                  │
                                                  ▼
        ┌──────────────── BACKEND (orchestrator) ─────────────────┐
        │  browse every PLC · read telemetry · route wires        │
        │  write setpoints · provision sim pods (kubectl)         │
        └──────┬───────────────────────────────────────┬─────────┘
        OPC-UA │                                        │ WebSocket
        ┌──────▼───────────┐  ┌──────────────┐    ┌─────▼──────────────┐
        │ sim PLC pods     │  │ real PLCs    │    │ frontend (3D floor)│
        │ (dynamic, k8s)   │  │ (by URI)     │    └────────────────────┘
        └──────────────────┘  └──────────────┘
```

---

## Where things live

| Path | What it is | Crate / stack |
|---|---|---|
| [`plant_config/`](plant_config/) | Shared schema: data types, device-type I/O contract, plant hierarchy. Types only. | Rust lib |
| [`backend/`](backend/) | Discovery, orchestration tick, CRUD + setpoint API, persistence, sim provisioning. | Rust · tokio · axum · sqlx · async-opcua (client) |
| [`simulator/`](simulator/) | Dumb physics host: self-builds from the API, runs Rhai physics, serves OPC-UA. | Rust · tokio · async-opcua (server) · rhai |
| [`frontend/`](frontend/) | 3D floor viewer; talks only to the backend API + WebSocket. | TypeScript · Vite · Three.js |
| [`codegen/`](codegen/) | Generates `helm/factory-sim/values.yaml` from config. | Rust bin |
| [`config/`](config/) | Seed `plant.json` + `device_types.json` (loaded into the DB once, then ignored). | JSON |
| [`helm/`](helm/), [`deploy/`](deploy/), [`Justfile`](Justfile) | k3s/Helm charts, machine provisioning, task runner. | Helm · bash · just |
| [`backend/migrations/`](backend/migrations/) | Postgres schema (sqlx migrations). | SQL |

---

## Documentation map

Start here, then follow a link to go deep on any one part.

| Doc | Read it to understand… |
|---|---|
| **[Architecture](docs/architecture.md)** | The whole system, the components, and the one **uniform tick** that moves every value. Start here. |
| **[Backend internals](docs/backend.md)** | Connector lifecycle (browse→write→poll), the orchestration tick, the Postgres LISTEN loop, the k8s reconciler. |
| **[Simulator internals](docs/simulator.md)** | Self-build, the OPC-UA address space (read-only outputs + writable inputs), the Jacobi tick + readiness gate, Rhai physics. |
| **[Data model](docs/data-model.md)** | Every Postgres table, how a plant round-trips DB ↔ runtime, the NOTIFY triggers. |
| **[API reference](docs/api.md)** | Every HTTP + WebSocket endpoint, with request/response shapes. |
| **[Deployment](docs/deployment.md)** | k3s-over-Tailscale, Helm, CI→GHCR→Keel, provisioning, and the operational gotchas. |
| **[Frontend](docs/frontend.md)** | How the 3D scene is built from config and driven by the live WebSocket stream. |
| [Design north-star](ARCHITECTURE.md) | The original target-state design rationale (the *why* behind each decision). |

Architecture diagrams live in [`docs/diagrams/`](docs/diagrams/) as `.excalidraw` files —
open them in the [Excalidraw VS Code extension](https://marketplace.visualstudio.com/items?itemName=pomdtr.excalidraw-editor)
or drag them onto [excalidraw.com](https://excalidraw.com).

---

## The one idea to take away

Everything is **browse-driven and uniform**. The backend never trusts config to know
what is on a PLC — it browses the live OPC-UA address space. Every wire (same-PLC or
cross-PLC) is read-then-written by the backend each tick. Every device reads *last*
tick's inputs (a **Jacobi step**), so causality falls out of the clock with no
dependency sorting and feedback loops just work. See [Architecture › The uniform tick](docs/architecture.md#the-uniform-tick).

---

## Quickstart (local, no cluster)

```bash
# 1. Backend — falls back to config/plant.json when DATABASE_URL is unset.
just be

# 2. A simulator process for one PLC (reads the same config files).
just sim plc-001
# …or one process per simulated PLC:
just sim-all

# 3. Frontend
cd frontend && npm install && npm run dev
```

With a database, set `DATABASE_URL` and `SEED_DIR=config`: the backend runs the
[migrations](backend/migrations/), seeds the DB from JSON once, and from then on the
DB is the only source of truth. See [Data model › Seeding](docs/data-model.md#seeding)
and [Deployment](docs/deployment.md).

**Key environment variables:** `DATABASE_URL`, `SEED_DIR`, `PLANT_CONFIG`, `BE_HOST`/`BE_PORT`
(default `0.0.0.0:3001`), `BE_TICK_MS`, `OPCUA_URI_OVERRIDE`, `SIMULATOR_IMAGE`, `K8S_NAMESPACE`,
`ASSET_DIR`, `PKI_DIR` (backend); `SIM_PLC_ID`, `BACKEND_URL`, `SIM_TICK_MS`, `OPCUA_HOST`
(simulator). Read ad-hoc via `std::env::var` across a few files; only `BE_HOST`/`BE_PORT`/`BE_TICK_MS`
are centralized (in `AppConfig`). Details in each component doc.
