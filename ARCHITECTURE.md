# Architecture

> **North-star — present tense, target state (2026 H2).** This describes the system we are building
> toward, written as if already true. The builder implements against it; step-by-step status lives in the
> plan (`deep-gathering-globe.md`), not here.

An industrial-plant digital twin you **build and run at runtime**. You author *device types* (a physics
model + inputs/outputs + a 3D look), assemble them into *PLCs* — simulated or real — place those on a
floor, and wire any device's output to any other device's input. The **backend discovers every PLC over
OPC-UA and runs the plant**: it reads telemetry, routes wired values, writes setpoints, and spins up
simulated PLCs on demand. Simulated and real PLCs are indistinguishable to it. Changing the plant is never
a file edit or a redeploy — it happens live through the API.

---

## Principles (and why)

- **OPC-UA is the only boundary.** Every PLC — sim or real — speaks OPC-UA; the backend is always the
  client. *Why:* a real PLC and a simulated one must be swappable with zero code difference.
- **Discover, never assume.** The backend learns a PLC's devices and metrics only by **browsing** its live
  address space — even for simulators. It never reads config to know what's on the wire. *Why:* the moment
  the backend "knows" a sim's internals, sim and real stop being identical.
- **The backend orchestrates; the edge does physics.** Physics runs inside the simulator (or in reality,
  for a real PLC). The backend never computes physics — it reads outputs, writes inputs and setpoints.
  *Why:* a real PLC runs its own behaviour; mirroring that keeps the twin honest.
- **All wiring is uniform and backend-routed.** Every wire — same PLC or across PLCs — is read-then-written
  by the backend each tick. *Why:* one mechanism, one consistent latency; lets you wire any device to any
  device; keeps simulators dumb.
- **Previous-tick step (Jacobi).** Every device reads *last* tick's inputs. *Why:* causality falls out of
  the clock — a value travels one hop per tick, so series chains react in order and parallel branches react
  together — with no dependency sorting and no cycle restriction (feedback loops just work).
- **The plant lives in a database, not files.** Postgres holds the libraries, instances and wiring; an
  object store holds 3D assets. *Why:* edit the plant live, with history; no redeploy.
- **Simulated PLCs are provisioned on demand.** The backend creates and destroys their pods through the
  Kubernetes API. *Why:* unlimited PLCs on a fixed set of nodes, bounded only by memory, created the
  instant a user adds one.

---

## Shape

```
  ┌ Device Library ┐   ┌ PLC Library ┐   ┌ Floor / wiring ┐    persisted in Postgres + object store
  │ physics · I/O  │ → │ sim or real │ → │ any → any wires│
  └────────────────┘   └─────────────┘   └───────┬────────┘
                                                  ▼
                      ┌──────────── BACKEND (orchestrator) ───────────┐
                      │ browse every PLC · read telemetry · route     │
                      │ wires · write setpoints · provision sim pods  │
                      └───┬───────────────────────────────┬───────────┘
                 OPC-UA   │                               │ k8s API
             ┌────────────▼───────────┐       ┌───────────▼──────────┐
             │ sim PLC pods (dynamic) │       │ real PLCs (by URI)   │
             │ inputs  = writable     │       │ browsed + written    │
             │ outputs = read-only    │       └──────────────────────┘
             └────────────────────────┘
                      │ WebSocket (live state)
                      ▼
                   frontend (floor editor + 3D)
```

---

## The uniform tick

How a value moves, every tick (default 100 ms, tunable):

1. **Read** — the backend polls each PLC's *browsed* output nodes into `IngestedState`.
2. **Route** — for every wire, it writes the upstream value into the downstream device's **writable input
   node** (the same write path setpoints use).
3. **Step** — each simulator reads its input nodes, runs physics on the frozen previous-tick snapshot, and
   writes its outputs. Order doesn't matter; a device with any unconnected input stays idle (**readiness
   gate**).
4. **Render** — the backend streams `IngestedState` to the frontend.

A change crosses a chain at **one device per tick**, so end-to-end latency is `depth × tick` — shrink the
tick for faster chains. (This is also truer to physics: pressure and flow take time to propagate.)

**This makes the backend load-bearing for physics coupling, not just observation:** if it stalls, wired
inputs freeze and devices go un-ready. That's the accepted cost of one uniform, honest data path.

---

## Persistence & libraries

Postgres is the source of truth for everything the user builds; the object store holds binaries. Normalised
tables (entities are reused and relationally wired) with JSONB for the schemaless parts.

```
deploy_nodes      finite cluster nodes a sim PLC can target — synced live from the k8s API
device_types      physics (Rhai) + io_spec (JSONB: inputs/outputs/params) + model/icon refs
plcs              sim (→ deploy_node) | real (→ endpoint_uri)
device_instances  a device_type placed in a PLC, with param values
wires             any output → any input (cross-PLC allowed)
discovered_nodes  cache of each PLC's browsed address space
audit_log         before/after history for rollback
```

- `deploy_nodes` mirrors the live cluster, so the PLC builder only offers real, Ready targets.
- 3D assets start on a local PVC behind an `AssetStore` seam (`*_ref` = a key), swappable for S3 later
  without touching callers. No binaries in Postgres.

---

## Simulators: dumb physics hosts

A simulator is a leaf process that **builds itself** from `GET /api/plc/{id}/config` — its own instances +
I/O — then exposes an OPC-UA server with read-only **output** nodes and writable **input** nodes. Each tick
it reads inputs, runs Rhai physics, writes outputs. It owns no topology and knows nothing of other PLCs —
the backend wires it. *(That config call is the sim's firmware, not backend discovery; the backend still
browses the sim like any real PLC.)*

---

## Dynamic provisioning

Adding a simulated PLC writes a row in `plcs`; the backend's reconcile loop creates the Deployment +
Service (and deletes them when the row goes), self-healing on restart. Each sim PLC is a ClusterIP
`plc-<id>:4840` reached over the cluster network — no host ports, no per-PLC LoadBalancer. Real PLCs need
no pod; the backend dials their URI.

---

## Deployment

k3s across a fixed set of machines (cloud + edge) joined by **Tailscale**, which doubles as the cluster
network — flannel rides the encrypted tunnel, so no firewall ports are opened. The backend holds a
tightly-scoped ServiceAccount to manage sim pods. Browser traffic reaches the frontend and backend through
**Tailscale Funnel**.

```
  browser ──HTTPS──► Tailscale Funnel ──► frontend (:443→8080)  ·  backend WS (:8443→3001)
                                          │
   ┌─ k3s, Tailscale-as-network ──────────┴────────────────────────────────────────┐
   │  backend ·· Postgres ·· frontend        sim PLC pods (ClusterIP plc-*:4840)    │
   │      └── browses/reads/writes ──────────► spread across nodes by deploy_node    │
   └─────────────────────────────────────────────────────────────────────────────────┘
```

`just helm-deploy` installs the **platform** (backend, frontend, Postgres, RBAC). The **plant itself** is
then built live through the API — never via Helm. CI cross-compiles amd64+arm64 to GHCR; Keel polls GHCR
and rolls Deployments when a new image lands.

**Operational facts that bite** (kept from hard-won experience):

- Funnel proxies HTTP/2, which can't upgrade WebSockets — the backend gets its own Funnel port (8443).
- nginx must resolve upstreams lazily via the CoreDNS IP, or it exits when a service isn't Ready yet.
- The k3s server node needs ≥ 2 GB free RAM; swap breaks control-plane timeouts and nodes go `NotReady`.
- A re-joined node loses custom labels — reapply `deploy_target` or its sim pods stay `Pending`.
- `local-path` PVCs are node-pinned; one bound while its node is `NotReady` never gets its directory.
- Image GC is tuned (60 %/40 %) so small edge disks don't fill from accumulated layers.

---

## Pieces

| Unit | Role |
|---|---|
| `plant_config` | Shared schema (DataType, device-type I/O contract) — types only, no runtime state. |
| `backend` | Discovery, orchestration tick, setpoint/CRUD API, persistence, sim provisioning. Async OPC-UA client. |
| `simulator` | Dumb physics host: self-builds, runs Rhai, serves OPC-UA. Async OPC-UA server. |
| `frontend` | Floor editor + 3D render; talks only to the backend API + WebSocket. |
| Postgres · object store | Plant state + assets. |

OPC-UA is the **async** `opcua` line — sessions reconnect internally, so browse/read/write share one
session with no manual reconnect handling.

---

## Backend API

| Endpoint | Purpose |
|---|---|
| `GET /ws` | live `IngestedState` stream |
| `… /api/device-types · /plcs · /instances · /wires` | CRUD the libraries, floor and wiring |
| `GET /api/plc/{id}/discovered` | a PLC's live browsed node tree |
| `POST /api/setpoint` | write a writable node (sim or real) |
| `GET /api/plc/{id}/config` | **sim self-build** — a simulator fetching its own definition (never for real PLCs) |
| `GET /api/assets/{id}` | a 3D model / icon |
| `GET /health` | liveness |
