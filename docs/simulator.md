# Simulator internals

Rust · tokio · async-opcua (server) · rhai. Entry point: [`simulator/src/main.rs`](../simulator/src/main.rs).

A simulator is a **dumb physics host**: one process per simulated PLC. It owns no topology
and knows nothing of other PLCs. It builds *itself* from the backend API, runs its devices'
physics each tick, and exposes an OPC-UA server. The backend wires it like any real PLC.

```
   ┌─ simulator process (SIM_PLC_ID = plc-001) ──────────────────────┐
   │                                                                 │
   │   loader   ── GET {BACKEND_URL}/api/plcs/{id}/config  (or files)│
   │                  → ResolvedPlant sliced to this one PLC          │
   │                                                                 │
   │   physics  ── Rhai scripts compiled once (PhysicsEngine)        │
   │   tick     ── Jacobi double-buffer, every SIM_TICK_MS           │
   │   server   ── OPC-UA: read-only outputs + writable inputs        │
   │   health   ── GET :9000/health  (k8s liveness)                  │
   └─────────────────────────────────────────────────────────────────┘
```

---

## Self-build (loader)

[`loader.rs`](../simulator/src/loader.rs). Two sources, picked by env:

```
  BACKEND_URL set  →  GET {BACKEND_URL}/api/plcs/{SIM_PLC_ID}/config
                      returns { plc, device_types }  (backoff-retried: 2,4,8,16,30s)
  else             →  read PLANT_CONFIG/{plant.json, device_types.json}
```

Either way the result is a `ResolvedPlant` (instance config merged with type definitions,
params validated) **sliced to exactly one PLC** — `plant.config.plcs[0]` is the one this
process owns. The config call is the simulator's "firmware"; it is *not* backend discovery
(the backend still browses the sim afterwards).

---

## OPC-UA address space

[`server/plc_server.rs`](../simulator/src/server/plc_server.rs). Built on `async-opcua`'s
`SimpleNodeManager`. Our namespace registers at index **2**, matching the
`ns=2;s={plc}.{device}.{field}` contract.

```
  ObjectsFolder
    └─ {plc_name}                         folder
         └─ {device_id}                   folder
              ├─ {metric}   read-only     ← output; physics writes, backend reads
              └─ {input}    WRITABLE       ← input port; backend writes wired values here
```

- **Output metric nodes** come from each device type's `metrics`. A background loop reads the
  `SimulatorState` snapshot each tick and pushes current values to these nodes.
- **Writable input nodes** come from each device's `input_variables`. Each gets a **write
  callback**: when the backend writes one over OPC-UA, the callback stores the value into
  `SimulatorState`, so the next Jacobi tick reads it from the frozen snapshot.

This is the mechanism that lets the backend route any wire into a simulated device.

---

## The Jacobi tick

[`tick.rs`](../simulator/src/tick.rs). Order-independent, double-buffered:

```
  1. snapshot = freeze previous-tick state          (all devices read from this)
  2. for each device (any order):
        if physics_mode == Live      → skip   (real-fed; backend drives it)
        if any declared input has no value in snapshot → skip   (readiness gate)
        run physics(device_type, state, params, dt)
        stage result
  3. commit all staged results                       (visible next tick)
```

Consequences (the same ones described in [architecture.md › The uniform tick](architecture.md#the-uniform-tick)):
no topo-sort, feedback loops allowed, chain latency = `depth × tick`. The **readiness gate**
keeps a device idle until every wired input is carrying a value — a device with no inputs is
vacuously ready.

---

## Physics (Rhai)

[`physics_definitions.rs`](../simulator/src/physics_definitions.rs). Each device type may carry
a `physics_definition` — a Rhai script, compiled once at startup. Per tick, per device, the
engine runs the script with three variables in scope:

```
  state   (Map)  device's current fields — mutate in place to produce outputs
  params  (Map)  this instance's tuning constants
  dt      (f64)  tick duration in seconds
```

After the run, updated `state` keys are read back and **type-coerced to match each field's
existing `DataType`** (Float/Bool/Str) before being committed. A type that has no script is a
pure pass-through.

---

## Configuration

| Env | Meaning | Default |
|---|---|---|
| `SIM_PLC_ID` | which PLC this process owns (required) | — |
| `BACKEND_URL` | fetch config from the API; unset → read files | unset |
| `PLANT_CONFIG` | config dir for the file path | — |
| `SIM_TICK_MS` | tick period | 100 |
| `OPCUA_HOST` | hostname advertised in discovery URLs | `HOSTNAME` / localhost |
| `PKI_DIR` | OPC-UA cert store base | `./pki` |
| `SIM_HEALTH_PORT` | liveness endpoint port | 9000 |

In the cluster these are set by the backend's reconciler when it creates the pod
(see [backend.md › k8s reconciler](backend.md#k8s-reconciler)).
