# Plan: Discover-and-orchestrate runtime plant
*(device library + PLC library + browse-based discovery + uniform backend wiring + dynamic sim provisioning)*

## Context

The product goal is a plant the user **builds and runs at runtime** from the frontend, with the backend
treating simulated and real PLCs identically. This supersedes the earlier "persist `plant.json` to
Postgres" plan — that becomes **Phase 1** of this larger epic.

**The core inversion.** Today the backend never discovers anything: [`endpoint_configs()`](plant_config/src/resolved.rs#L109)
*computes* OPC-UA node IDs (`ns=2;s={plc}.{device}.{metric}`) from `device_types.json`, and
[`ScadaPlcConnector`](backend/src/comms/connectors/scada_plc_connector.rs) `session.read()`s that fixed
list. There is **no Browse and no Write anywhere in the codebase** — it is a static, read-only poller.

The new model: the backend **always browses** every PLC endpoint (sim or real) to *learn* its device/metric
tree, renders it, lets the user **wire devices across PLCs**, and at runtime **routes values and writes
setpoints** over OPC-UA. Authored device instances exist only to *drive a simulator's address space*; the
backend's source of truth for "what's on a PLC" is the live Browse. This is the strongest expression of the
project's "indistinguishable from a real PLC" goal — sim and real now share one discovery+control path.

## Confirmed decisions

1. **Dynamic sim provisioning → backend via k8s API (RBAC).** Adding a simulated PLC to the library makes
   the backend create the Deployment+Service. Truly dynamic, memory-bound count of PLCs per fixed node set.
2. **Wiring → uniform backend routing (all wires, not just cross-PLC).** The backend reads every upstream
   output and writes it into the downstream device's input each tick → it is the simulation orchestrator.
   Every device reads *previous-tick* inputs (synchronous Jacobi step): ordering/causality emerges from the
   tick clock (a value travels one hop per tick), so **no topo-sort** is needed and feedback loops just work.
   The intra/cross-PLC distinction disappears — it was only ever a transport detail.
3. **Storage → normalized entity tables + object store.** Postgres tables with JSONB for the schemaless
   bits; 3D models/icons in object storage (local PVC behind `AssetStore` seam now, S3 later).
4. **OPC-UA Write is in scope now.** Setpoint control + the cross-PLC injection path share one write path.

## Target architecture

```
  Device Library ──┐                         ┌── Postgres (entities + JSONB) ── object store (assets)
  (DeviceTypes:    │   build at runtime       │
   physics, I/O,   ├─► PLC Library ─► Floor/Topology ─► wiring (any device → any device)
   model/icon)     │   (sim: node+        (place PLCs, wire discovered devices)
                   │    instances /
                   │    real: OPC-UA URI)
                   ▼
        ┌──────────────── BACKEND (orchestrator) ───────────────────────────┐
        │ • reconciles sim PLC pods via k8s API (kube crate, RBAC)           │
        │ • per PLC: connect → BROWSE → poll(read) + WRITE  (one session)    │
        │ • orchestration tick: read upstream → write into downstream inputs │
        │ • setpoint API → write to real/sim nodes                           │
        └───────────────────────────────────────────────────────────────────┘
              │ OPC-UA (read/browse/write)              ▲ create/delete Deployments+Services
              ▼                                         │
   ┌─ sim PLC pods (dynamic) ─┐   ┌─ real PLCs (URI) ─┐ │
   │ writable input nodes +   │   │ browsed + written │ │
   │ output metric nodes      │   └───────────────────┘ │
   │ fetch instances from API │◄──────────────────────────┘
   └──────────────────────────┘
```

**Wiring & physics-step semantics.** *All* wires route through the backend uniformly: each tick it reads the
upstream output from `IngestedState` and `session.write()`s it into the downstream device's **writable input
node** (same path as setpoints). Every device therefore consumes **previous-tick** inputs — a synchronous
**Jacobi step**. Consequences: a change travels **one hop per tick**, so series chains propagate *in order*
(valve→fm1 reacts, then fm1→fm2 the next tick) while parallel branches off one source update *together*;
**no topo-sort** is needed (processing order within a tick is irrelevant when all inputs are frozen
previous-tick); and **feedback loops are allowed** (they iterate tick-by-tick — the old Kahn sort rejected
them). Cost: end-to-end latency along a chain = `depth × tick`; shrink the tick for faster chains. (This is
also *more* faithful for pressure/flow — propagation genuinely takes time rather than being instantaneous.)

**Readiness gate (new).** A simulated device runs physics only when **all its declared input ports have a
value** (i.e. wired and carrying a previous-tick value). Input ports become first-class (distinct from
output metrics and from tuning `params`); an unconnected input keeps the device idle.

## Data model (Postgres + object store)

```
deploy_nodes(name pk, display_name, k8s_node, arch, mem_allocatable_mb, status, last_seen)
   -- finite set of cluster nodes a simulated PLC can target; matches the `deploy_target` node label.
   -- Synced from the live k8s API (labels + allocatable resources), not hand-authored — stays truthful.
device_types(id, name, physics_rhai text, io_spec jsonb, model_ref, icon_ref, updated_at)
   io_spec = { inputs:[{name,type}], outputs:[{name,type,initial}], params:[{name,default}] }
plcs(id, name, kind enum('simulated','real'), deploy_node fk→deploy_nodes?, endpoint_uri text?, updated_at)
   -- deploy_node set for simulated PLCs (the chosen target node); null for real PLCs (use endpoint_uri).
device_instances(id, plc_id fk, device_type_id fk, name, param_values jsonb, floor_pos jsonb?)
wires(id, src_plc_id, src_node, dst_instance_id, dst_input_port)          -- cross-PLC allowed
discovered_nodes(plc_id fk, node_id, browse_path, browse_name, datatype, access_level, last_seen)
   -- OPC-UA nodes (distinct from deploy_nodes, which are cluster machines).
audit_log(id, entity, entity_id, before jsonb, after jsonb, at)           -- change history/rollback
```
- **JSONB** for the genuinely schemaless bits (`io_spec`, `param_values`, `floor_pos`); **columns + FKs**
  for the entities that are reused/queried/wired. (This revises the earlier single-document call: library
  reuse + relational wiring now justify normalization.)
- **Assets** (3D/icons) start on a **local PVC** (`/data/assets/<id>`) behind an `AssetStore` trait seam
  (`*_ref` = a relative key); swap to S3 later without touching callers. No binaries in Postgres.
- **History/rollback** via `audit_log` (before/after per change) + per-entity restore. Full point-in-time
  snapshot rollback is deferred (temporal-table territory).

## Connector changes  ([generic_connector.rs](backend/src/comms/generic_connector.rs), [scada_plc_connector.rs](backend/src/comms/connectors/scada_plc_connector.rs))

- **Invariant — browse everything, no config shortcut.** The backend learns *every* PLC's devices/metrics
  by Browse — **sim PLCs included**, discovered identically to real ones. It never reads device config to
  know what to poll/render/wire. (`GET /api/plc/{id}/config` is the *simulator building itself*, a separate
  actor — not the backend discovering it.) **Interpretation** of a discovered node (icon, DeviceType,
  semantics) is a separate **mapping layer** applied uniformly: auto-derived for sims, user-assisted for
  real PLCs; the poll/render/wire path stays browse-driven for both.
- Extend `ConnectorImpl`: add `browse(&conn) -> DiscoveredTree` (`session.browse()` walking from
  ObjectsFolder; capture node_id, browse_name, datatype, **AccessLevel** → writable = setpoint-capable)
  and `write(&conn, node_id, value)` (`session.write()`).
- Lifecycle per PLC: **connect → browse → build read set from browse (not `endpoint_configs()`) → poll**.
  The run loop also drains an inbound `mpsc::Receiver<WriteCmd>` each tick (orchestration + setpoints),
  writing before it reads. One session serves read+browse+write. Re-browse on reconnect.
- Built on the **async opcua** session from Phase 0 — which removes the 0.12 shared-runtime reconnect
  gotcha entirely (async sessions reconnect internally), so browse/write/poll run on it without the
  `drop(conn)` hack.

## Backend orchestration + APIs

- **Reconcile loop (kube crate, RBAC):** diff desired sim PLCs (`plcs` table) vs actual Deployments;
  create/delete Deployment+Service+PVC. Sets `nodeSelector` from `plcs.deploy_node` → `deploy_nodes.k8s_node`,
  and rejects/warns when a node's planned PLCs exceed `deploy_nodes.mem_allocatable_mb` (the memory-bound
  count). Each sim PLC = ClusterIP `plc-<id>:4840` (backend reaches it via CoreDNS over flannel; no per-PLC
  port juggling, no LoadBalancer). Self-heals on backend restart.
- **Node sync:** the same kube client refreshes `deploy_nodes` from live cluster nodes (their `deploy_target`
  label, arch, allocatable memory, Ready status) so the PLC-library node picker only offers real targets.
- **Orchestration tick:** every `tick_ms`, for **each** `wire`, look up the upstream value in
  `IngestedState` and enqueue a `WriteCmd` to the downstream connector targeting its input node. Uniform —
  no intra/cross branching; all inputs are previous-tick (Jacobi).
- **HTTP API (axum, [ws_bridge.rs](backend/src/api/ws_bridge.rs)):** CRUD for device types / PLCs /
  instances / wires; `GET /api/plc/{id}/discovered` (browse tree); `POST /api/setpoint` (write);
  `GET /api/plc/{id}/config` (**sim self-build** — a simulator fetches its own instances + writable-input
  spec to construct itself; *not* backend discovery, and never called for real PLCs). Config held in a
  `watch`-style snapshot; reload on change.

## Simulator changes  ([loader.rs](simulator/src/loader.rs), [plc_server.rs](simulator/src/server/plc_server.rs), [tick.rs](simulator/src/tick.rs))

- Fetch instances from `GET /api/plc/{SIM_PLC_ID}/config` (backend API) instead of the ConfigMap; reuse
  existing backoff.
- Address space gains a **writable input-port node** for *every* device input (the backend writes these);
  output metrics stay read-only. An inbound write updates `SimulatorState`.
- Tick becomes uniform and **order-independent**: double-buffer (compute every device from the frozen
  previous-tick snapshot, then commit), read inputs from the writable nodes, run physics, write outputs.
  **Drop the topo-sort / `TickPlan`** ([tick.rs](simulator/src/tick.rs)) — previous-tick semantics make
  ordering moot and enable feedback loops. **Gate physics on readiness** (all input ports valued).
  `physics_mode: Live` still skips physics for real-fed devices.

## Existing logic that must change (paths)
- Connector trait + impl (browse/write/command queue) — see above.
- `endpoint_configs()` ([resolved.rs:109](plant_config/src/resolved.rs#L109)) — no longer feeds the
  backend; stays only to build the *simulator's* address space.
- Cross-PLC validation ([resolved.rs:61-76](plant_config/src/resolved.rs#L61)) — **removed**: any device may
  wire to any other (resolved by the backend, not the physics engine).
- Topo-sort / `TickPlan` ([tick.rs](simulator/src/tick.rs)) — **removed**: replaced by an order-independent
  double-buffered Jacobi tick; cycle rejection goes away (feedback loops now supported).
- `plant_config` schema — DeviceType gains an explicit I/O contract (inputs vs outputs vs params); add
  `kind: simulated|real`; entities move to the DB.
- Simulator deployment — [simulator.yaml](helm/factory-sim/templates/simulator.yaml) static
  `range .Values.plcs` is dropped; the backend creates sim pods. `gen_helm_values.rs` PLC enumeration
  becomes obsolete (keeps only backend/frontend values).

## Stack additions
- `sqlx` (Postgres) · `kube` + `k8s-openapi` (sim reconcile) · `async-opcua` (≥0.13) ·
  local PVC asset store (→ S3 later). New cluster pieces: Postgres StatefulSet+PVC; backend
  ServiceAccount + Role/RoleBinding (Deployments/Services/PVCs).

## Phasing (each phase ships independently)
0. **Migrate to async opcua** (`async-opcua` / opcua ≥0.13) — do this *first*. Rewrites the two OPC-UA
   files: [scada_plc_connector.rs](backend/src/comms/connectors/scada_plc_connector.rs) (client →
   `session.read(...).await`, spawn the session event loop) and
   [plc_server.rs](simulator/src/server/plc_server.rs) (server → new **node-manager** address-space API;
   this is the bulk). Ripples: [`ConnectorImpl`](backend/src/comms/generic_connector.rs#L13) becomes
   `async fn`; connectors become tokio tasks not `std::thread`; [plant.rs](backend/src/plant.rs) spawns
   tasks. **Deletes** the `drop(conn)` reconnect hack + most of the backoff loop (async sessions reconnect
   internally), and makes `browse`/`write`/subscriptions first-class — which is why it precedes Phases 2–3.
   Size: medium (~1–2 days, server half is the fiddly part). Task one: confirm the exact crate/version.
1. **Persistence + libraries** — Postgres + entity schema + asset store (local PVC + `AssetStore` trait) +
   CRUD APIs + seed from current `device_types.json`/`plant.json` and `deploy/inventory.json` →
   `deploy_nodes`. (Was the old plan; now revised to entities.) PLC-library node picker reads `deploy_nodes`.
2. **Browse discovery** — connector `browse()`, `discovered_nodes` cache, discovery API; backend read set
   from browse. Sim PLCs still static here to isolate the discovery change.
3. **Write path** — connector `write()` + command queue + `POST /api/setpoint`.
4. **Wiring orchestration + readiness** — backend routes *all* wires each tick (read→write); sim exposes
   writable input nodes for every input + double-buffered Jacobi tick (drop topo-sort) + readiness gate;
   remove cross-PLC validation.
5. **Dynamic sim provisioning** — backend RBAC + `kube` reconcile from `plcs`; sim fetches from API; drop
   the static per-PLC helm range.
6. *(Later)* Frontend floor editor + asset upload.

Order: 0 → 1 → 2 → 3 → 4 (needs 3) and 5 (needs 1); 5 can run parallel to 2–4. Phase 0 and Phase 1 are
independent and can run in parallel (different files).

## Risks / call-outs
- **Backend RBAC** to create workloads is a real privilege; scope the Role tightly to its namespace.
- **Writing to real PLCs** is real-world control — guard setpoint writes (range checks, explicit confirm).
- **Browse heuristics** vary by server; mapping a real PLC's raw nodes → DeviceTypes (for icons/physics) is
  user-assisted, not automatic.
- **Chain-depth propagation latency** (`depth × tick`, one hop per tick) is by design — the tick rate is the
  lever. Document it so it isn't mistaken for a bug.
- **Backend is now load-bearing for physics coupling**, not just observation: a backend stall freezes all
  wired inputs (devices go un-ready). Acceptable (it's already critical path), but note it.

## Verification (per phase, end-to-end)
1. Seed: tables populated from current JSON; `GET` libraries match today's plant; existing stack unchanged.
2. Browse: `GET /api/plc/{id}/discovered` returns the live node tree for a sim PLC (and a real one if
   available); backend polls the *discovered* nodes, not config-derived ones.
3. Write: `POST /api/setpoint` flips a writable node; value confirmed by re-read/browse.
4. Wiring: wire device-A output → device-B input (same or different PLC); B stays idle until wired
   (readiness), then runs off A's value. Verify a series chain A→B→C propagates one hop per tick (B reacts
   before C), and two siblings off A update on the same tick. Unwire → idles again.
5. Provisioning: add a simulated PLC via API → backend creates the pod/Service; it appears, is browsed, and
   renders. Delete via API → pod/Service/PVC removed. Kill the backend → reconcile restores desired state.
