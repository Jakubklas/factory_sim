# Backend internals

Rust · tokio · axum · sqlx · async-opcua (client). Entry point: [`backend/src/main.rs`](../backend/src/main.rs).

The backend is the orchestrator: it browses and polls every PLC, routes every wire,
writes setpoints, serves the API, persists to Postgres, and provisions simulated PLC pods.

---

## Configuration

Every backend env var is read in **one place** — `AppConfig::from_env`
([`config_handle/app_config.rs`](../backend/src/config_handle/app_config.rs)) — which declares
each variable, its default, and its meaning. `main` loads it once into an `Arc<AppConfig>` and
passes it (or the fields a component needs) down; nothing else calls `std::env::var`. So
threading config to a connector means adding a field to `AppConfig` and a parameter, not a new
scattered `env::var`. Full annotated list: [`.env.example`](../.env.example).

---

## Startup sequence

[`main.rs`](../backend/src/main.rs) wires everything together, then runs until SIGTERM:

```
  1. connect Postgres            (DATABASE_URL; optional — falls back to plant.json)
  2. seed if empty               (SEED_DIR — one-time JSON → DB import)
  3. load plant from DB          (db::plant_loader::load → PlantConfig)
  4. start connectors            (plant::start → one tokio task per PLC)
  5. load wires + start tick     (orchestration::start)
  6. spawn LISTEN loop           (react to plant_changed NOTIFY)
  7. spawn k8s reconciler        (sim pod create/delete + node sync)
  8. spawn API/WebSocket server  (axum)
```

Each of 4–8 is an independent tokio task sharing a few `Arc` handles:

| Shared state | Type | Who writes | Who reads |
|---|---|---|---|
| `ingested` | `Arc<RwLock<IngestedState>>` = `device_id → field → DataType` | connectors (poll) | orchestration, WS stream |
| `write_handles` | `Arc<RwLock<HashMap<plc_name, WriteHandle>>>` | plant/listen (insert) | orchestration, setpoint API |
| `discovered` | `Arc<RwLock<DiscoveredState>>` = `plc_name → [BrowsedNode]` | connectors (browse) | `/api/.../discovered` |
| `wires` | `Arc<RwLock<Vec<OrchestratorWire>>>` | listen loop (refresh) | orchestration tick |

---

## Connectors — the protocol layer

One **`GenericConnector<C: ConnectorImpl>`** runs per PLC as a tokio task
([`comms/generic_connector.rs`](../backend/src/comms/generic_connector.rs)). The generic
half owns the lifecycle and backoff; the protocol half (`ConnectorImpl`) is swappable.
Today the only impl is **`ScadaPlcConnector`** (OPC-UA,
[`comms/connectors/scada_plc_connector.rs`](../backend/src/comms/connectors/scada_plc_connector.rs)).

```
            ┌──────────────── GenericConnector::run() ────────────────┐
            │                                                         │
   connect ─┤  connect_with_backoff()   (1,2,4,8,16,30s)             │
            │        │                                                │
   browse  ─┤  do_browse()  → DiscoveredState + internal poll list   │
            │        │                                                │
   loop ────┤  every tick_ms:                                        │
            │     drain write_rx → impl.write(cmd)   (setpoints/wires)│
            │     impl.poll()    → merge into IngestedState           │
            │     on repeated poll failure → reconnect + re-browse    │
            └─────────────────────────────────────────────────────────┘
```

The `ConnectorImpl` trait (implement this to add a protocol):

```rust
async fn connect(&self) -> Result<Self::Conn, …>;          // one attempt; generic retries
async fn browse(&self, &Conn) -> Result<Vec<BrowsedNode>, …>;  // learn the address space
async fn write (&self, &Conn, &WriteCmd) -> Result<(), …>;     // before each poll
async fn poll  (&self, &Conn) -> Result<PartialState, …>;      // read all browsed vars
```

### Browse is the source of truth for reads

`ScadaPlcConnector::browse()` does an iterative BFS (Breadth-First Search) from `ObjectsFolder` (depth ≤ 6) over
hierarchical references, then batch-reads the `DataType` attribute of every Variable node.
Each variable's browse path (`PLC/device_id/metric`) is parsed into `device_id` + `field`,
forming the internal poll list. **Reads never come from config** — only from what was browsed.
`poll()` batch-reads the `Value` of that list in one round-trip and returns a `PartialState`.

`WriteHandle` is a cheap `mpsc::Sender<WriteCmd>` clone; `send()` is non-blocking and drops
the command if the 256-deep queue is full (telemetry is more valuable than a stale setpoint).

---

## Orchestration tick

[`orchestration.rs`](../backend/src/orchestration.rs). Every `tick_ms`, for each wire:

```
  OrchestratorWire { src_device_id, src_field, dst_plc_name, dst_node_id }
        │
        ├─ value = IngestedState[src_device_id][src_field]    (skip if absent)
        ├─ handle = write_handles[dst_plc_name]               (skip if absent)
        └─ handle.send(WriteCmd { node_id: dst_node_id, value })
```

`dst_node_id` is pre-computed at load time as `ns=2;s={dst_plc}.{dst_device}.{input_port}`.
This is the single mechanism behind all wiring — same-PLC and cross-PLC are identical here.
The wire table is an `Arc<RwLock<Vec<…>>>`; the LISTEN loop swaps its contents atomically so
the next tick picks up edits with no restart.

---

## Postgres LISTEN loop

[`main.rs › run_listen_loop`](../backend/src/main.rs). Reacts to plant edits in near-real-time:

```
  LISTEN plant_changed
  on NOTIFY:
     orchestration::refresh_wires(pool, wires)     ← reload wire table
     plant_loader::load(pool) → for each PLC:
        plant::add_plc_connector(...)              ← start connector if new (idempotent)
     reconcile_kick.notify_one()                   ← wake the k8s reconciler now
```

Triggers fire on `plcs`, `device_instances`, and `wires`
(see [data-model.md › Triggers](data-model.md#notify-triggers)). `add_plc_connector` is a
no-op when a connector for that PLC name already exists. The final `notify_one()` is the
**reconcile kick** (a shared `tokio::sync::Notify`): it nudges the k8s reconciler so a
newly-added simulated PLC gets its pod within ~1 s instead of waiting up to its 15 s poll.
Bursts of edits coalesce to at most one extra reconcile.

---

## K8s reconciler

[`k8s.rs`](../backend/src/k8s.rs). Keeps simulated-PLC pods in sync with the `plcs` table.
It **shells out to `kubectl`** (no `kube` crate dependency); if `kubectl` isn't reachable it
logs and skips, so the backend runs fine outside a cluster. Two loops:

```
  reconcile (15s OR on kick):  desired = SELECT … FROM plcs WHERE kind='simulated'
                    actual  = kubectl get deployments -l managed-by=factory-sim
                    create missing  → kubectl apply  (Deployment + Service + PVC)
                    delete stale    → kubectl delete

  node-sync (30s):  kubectl get nodes → upsert deploy_nodes
                    (deploy_target label, arch, allocatable memory, Ready status)
```

Each sim pod is a `ClusterIP` service `plc-<id>:4840`; `nodeSelector` comes from the PLC's
`deploy_node`. Deleting a deployment lets k8s garbage-collect its ReplicaSet and Pods.
node-sync keeps `deploy_nodes` truthful so the PLC builder only offers real, Ready targets.

The reconcile is **level-triggered**: it polls because the actual state lives in Kubernetes
and drifts for reasons Postgres never emits (pod crashes, node loss, manual `kubectl`, a
failed apply), so each cycle re-asserts desired state and self-heals. The periodic poll is
the safety net; the LISTEN-loop **kick** above is the fast path for user edits. (node-sync
must poll regardless — its source is the cluster, not the DB.)

---

## API server

[`api/ws_bridge.rs`](../backend/src/api/ws_bridge.rs) builds the axum router and holds the
shared `AppState`. Full endpoint reference: **[api.md](api.md)**. Notable handlers:

- `stream_state` — the `/ws` loop: every tick, serialize `IngestedState` to JSON and push.
- `write_setpoint` — `POST /api/setpoint`: look up the PLC's `WriteHandle`, enqueue a `WriteCmd`
  (same path wires use).
- `sim_config` — `GET /api/plcs/:id/config`: a simulator fetching its own definition to self-build.
- `log_level_handler` — `GET /api/log-level?set=…`: live `RUST_LOG` reload via a tracing reload handle.

---

## No-database mode

With `DATABASE_URL` unset the backend reads `PLANT_CONFIG/plant.json`, derives the wire table
from each device's `input_variables`, and skips seeding, the LISTEN loop, and the reconciler.
Useful for local connector/physics work without standing up Postgres.
