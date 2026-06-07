# Architecture

How the pieces fit and how a single value travels end-to-end. Read this first;
each section links to the component doc that goes deeper.

> Interactive diagram: [`docs/diagrams/system-architecture.excalidraw`](diagrams/system-architecture.excalidraw).

---

## Components

```
                            ┌───────────────────────────────────────┐
   browser ── HTTP/WS ─────►│ frontend  (Three.js 3D floor)         │
                            └───────────────────────────────────────┘
                                          │  GET /api/plant   ·   /ws stream
                                          ▼
   ┌──────────────────────────── BACKEND ─────────────────────────────────┐
   │                                                                       │
   │   axum API + WebSocket   ──   CRUD libraries · setpoints · live state │
   │            │                                                          │
   │   one GenericConnector per PLC   (tokio task: connect→browse→loop)    │
   │            │  reads → IngestedState        writes ← WriteHandle queue │
   │   orchestration tick   — for each wire: read upstream, write input    │
   │   Postgres LISTEN loop — react to plant edits without a restart       │
   │   k8s reconciler       — create/delete sim pods via `kubectl`         │
   │                                                                       │
   └──────┬───────────────────────────┬────────────────────────┬──────────┘
   OPC-UA │                    sqlx    │                 kubectl │
   ┌──────▼─────────────┐    ┌─────────▼────────┐      ┌─────────▼─────────┐
   │ sim PLC (simulator)│    │   Postgres       │      │  k8s API (k3s)    │
   │ + real PLCs        │    │ (source of truth)│      │  Deployments/Svcs │
   └────────────────────┘    └──────────────────┘      └───────────────────┘
```

| Component | Responsibility | Deep dive |
|---|---|---|
| **plant_config** | Shared types: `DataType`, `DeviceTypeDefinition` (I/O contract), `PlantConfig`/`PlcConfig`/`DeviceConfig`. No runtime state. | — |
| **backend** | Browse + poll every PLC, route wires, write setpoints, serve the API, persist to Postgres, provision sim pods. | [backend.md](backend.md) |
| **simulator** | One process per simulated PLC. Self-builds from the API, runs physics, exposes an OPC-UA server. | [simulator.md](simulator.md) |
| **frontend** | Render the floor in 3D from `/api/plant`, animate it from the `/ws` stream. | [frontend.md](frontend.md) |
| **Postgres** | The plant: libraries, instances, wiring. Single source of truth. | [data-model.md](data-model.md) |

---

## Design principles (and why)

- **OPC-UA is the only boundary.** Every PLC — sim or real — speaks OPC-UA; the
  backend is always the client. *So a real PLC and a simulated one are swappable with
  zero code difference.*
- **Discover, never assume.** The backend learns a PLC's devices/metrics by **browsing**
  its live address space, even for simulators. It never reads config to know what's on
  the wire. *The moment the backend "knows" a sim's internals, sim and real stop being identical.*
- **The backend orchestrates; the edge does physics.** Physics runs inside the simulator
  (or in reality, for a real PLC). The backend only reads outputs and writes inputs/setpoints.
- **All wiring is uniform and backend-routed.** Every wire — same-PLC or cross-PLC — is
  read-then-written by the backend each tick. *One mechanism, one latency, any-to-any wiring.*
- **Previous-tick step (Jacobi).** Every device reads *last* tick's inputs. *Causality
  falls out of the clock — no dependency sort, and feedback loops just work.*
- **The plant lives in a database, not files.** *Edit live, with history; no redeploy.*
- **Simulated PLCs are provisioned on demand** through the k8s API. *Unlimited PLCs on a
  fixed node set, created the instant a user adds one.*

---

## The uniform tick

This is the heart of the system. Default period **100 ms** (`BE_TICK_MS` / `SIM_TICK_MS`).
Three loops run on this clock, each independently:

```
  ┌─ per-PLC connector loop (backend) ───────────────────────────────┐
  │  every tick:  drain write queue → session.write() each cmd        │
  │               session.read() all browsed nodes → IngestedState    │
  └──────────────────────────────────────────────────────────────────┘

  ┌─ orchestration loop (backend) ───────────────────────────────────┐
  │  every tick:  for each wire                                       │
  │     read  IngestedState[src_device][src_field]   (prev tick)      │
  │     send  WriteCmd → dst PLC's WriteHandle  (node = input port)   │
  └──────────────────────────────────────────────────────────────────┘

  ┌─ physics loop (simulator) ───────────────────────────────────────┐
  │  every tick:  freeze snapshot → run Rhai per device → commit      │
  │     (a device runs only when ALL its inputs have a value)         │
  └──────────────────────────────────────────────────────────────────┘
```

Putting it together — how one value moves from boiler to flow meter:

```
  tick N      boiler physics writes  pressure = 3.0   (sim, output node)
  tick N+1    backend reads pressure → IngestedState
  tick N+2    orchestration writes 3.0 → valve.inlet_pressure  (sim input node)
  tick N+3    valve physics consumes inlet_pressure, writes outlet_pressure
  tick N+4    backend reads it → … and so on down the chain
```

A change crosses a chain at **one device per tick**, so end-to-end latency is
`depth × tick`. Shrink the tick for faster chains. This is the accepted cost of one
uniform, honest data path — and it's truer to physics (propagation takes time).

> Interactive diagram: [`docs/diagrams/data-flow.excalidraw`](diagrams/data-flow.excalidraw).

**Why Jacobi (previous-tick) matters:**
- No topo-sort — processing order within a tick is irrelevant; all inputs are frozen.
- Feedback loops are legal — they iterate tick-by-tick instead of being rejected.
- Parallel branches off one source update on the same tick; series chains update in order.

---

## Live editing — no restarts

Editing the plant is an API write to Postgres. A trigger fires `pg_notify('plant_changed')`;
the backend's LISTEN loop reloads the wire table and starts connectors for any new PLC.
The k8s reconciler (separate 15 s loop) creates/deletes sim pods to match the `plcs` table.

```
  POST /api/plcs        ─►  INSERT plcs      ─► NOTIFY ─► LISTEN loop: add connector
  POST /api/wires       ─►  INSERT wires     ─► NOTIFY ─► LISTEN loop: refresh wires
  (simulated PLC row)   ─►  reconciler (15s)             ─► kubectl apply Deployment+Svc
```

See [backend.md › LISTEN loop](backend.md#postgres-listen-loop) and
[backend.md › k8s reconciler](backend.md#k8s-reconciler).

---

## Where the contract lives: OPC-UA node paths

Both sides agree on one node-id format, namespace index **2**:

```
   ns=2;s={plc_name}.{device_id}.{field}
                                   │
              output metric (read-only)  ── simulator publishes, backend reads
              input port    (writable)   ── backend writes, simulator consumes
```

The simulator builds these nodes from its device I/O spec; the backend *rediscovers*
them by browsing (it does not assume the format for reads — it walks the tree and
derives `device_id`/`field` from the browse path). Setpoint and wire writes target the
writable input node directly. See [simulator.md › Address space](simulator.md#opc-ua-address-space).
