# Architectural State & Direction: factory-sim

---

## Where the project is now

### Core architecture
- **4 Rust crates + TypeScript frontend** in a monorepo
  - `plant_config` — pure schema/validation, no I/O, no runtime state; shared by simulator + backend
  - `simulator` — one process per PLC; Rhai scripting for physics; OPC-UA server per process
  - `backend` — axum HTTP/WS server; one std::thread per OPC-UA connector; shared `IngestedState` map
  - `codegen` — single binary (`gen_helm_values`) that reads JSON configs and emits `values.yaml`
  - `frontend` — Vite + TypeScript + Three.js; subscription model; hardcoded 3D mesh per device type

### Config model
- `plant.json` — plant topology: PLCs → devices, wiring, ports, deploy targets
- `device_types.json` — device type registry: physics scripts (Rhai), metrics, functions, params
- Both loaded at startup; no runtime mutation; no DB
- Config baked into Kubernetes ConfigMap at deploy time via `--set-file`

### Data flow (current)
- Simulator writes `SimulatorState` (RwLock) → OPC-UA address space every `tick_ms`
- Backend polls OPC-UA per PLC (std::thread, exponential backoff) → `IngestedState` (RwLock)
- Backend WS bridge snapshots `IngestedState` every `tick_ms` → JSON to all connected browsers
- Frontend deserializes JSON → per-device subscribers → Three.js mesh/label updates

### Physics engine
- Rhai scripts stored as strings in `device_types.json`, compiled to AST at simulator startup
- Executed per device per tick with `state`, `params`, `dt` in scope
- Topological sort (Kahn's algorithm) resolves intra-PLC device dependencies before each tick
- Cross-PLC device dependencies not supported — must go via OPC-UA explicitly

### Deployment
- k3s (lightweight Kubernetes) on ≥2 nodes connected by Tailscale (flannel over Tailscale interface)
- Helm chart at `helm/factory-sim/`; values auto-generated from plant.json + platform.json + inventory.json
- OPC-UA security: anonymous, no TLS (endpoints accept any client)
- Backend API: no auth on any endpoint
- `just helm-deploy` from laptop reaches all nodes in one command

### Dependency snapshot
- `opcua 0.12` (locka99/opcua) — the only pure-Rust OPC-UA implementation; not async-native
- `axum 0.7` + `tokio 1.41` — standard async stack
- `rhai 1` with `sync` feature
- `three.js 0.170.0` + `vite 6.0` + `typescript 5.7`
- OpenSSL vendored for cross-compilation to arm64

---

## Critical architectural evaluation

### Strengths
- **Clean OPC-UA boundary** — simulator/real PLC are fully swappable; backend is protocol-agnostic at runtime
- **plant_config as schema contract** — no duplication between binaries; type-safe config parsing catches errors at startup, not runtime
- **Per-PLC process isolation** — one crashed simulator can't corrupt others; horizontal scale is trivial (new process, new port)
- **Rhai for physics** — hot-swappable device behaviour without recompiling; scripts are version-controllable config
- **Topological sort for device wiring** — correct execution order guaranteed; cycle detection at startup, not runtime
- **k3s + Tailscale** — zero exposed ports, encrypted cluster network, stable MagicDNS hostnames; correct for edge
- **Immutable `ResolvedPlant`** — built once, Arc-cloned everywhere; no lock contention on the hot path

### Weaknesses

**Protocol layer**
- OPC-UA polling introduces up to `BE_TICK_MS` stale-data latency; with 10+ PLCs this becomes a bottleneck
- **`opcua 0.12` shared-runtime bug** ⚠️ — the library wraps an async runtime behind a sync API and shares it across `Client` instances. When a `PlcConnection` is dropped while a new one is being established, the old client's shutdown tears down shared runtime state, closing the new session's message sender and causing every reconnect to fail with `BadSessionIdInvalid` + "Send message will fail because sender has been closed". Workaround: explicitly `drop(conn)` before calling `connect_with_backoff()`. **Real fix: upgrade to an async-native OPC-UA client** (e.g. `open62541` bindings or a future async fork of `opcua`). This is the most operationally disruptive known bug — any OPC-UA session hiccup (pod restart, network blip) triggers an infinite reconnect loop that only resolves with a full pod restart. Tracking this as a **must-fix before production**.
- `opcua 0.12` is old — the OPC-UA server blocks an OS thread per PLC and doesn't compose with tokio; no async server alternative in pure Rust yet
- OPC-UA security is disabled (anonymous, no signing/encryption) — fine inside Tailscale, but a single misconfiguration away from exposure
- `protocol` field on `PlcConfig` is always `"opcua"` — the abstraction is a fiction; `GenericConnector` trait exists but only one impl

**State management**
- `IngestedState` is in-memory only — process restart loses all telemetry; no persistence whatsoever
- No time-series storage: there is no way to query "what was temperature at 14:23?" — all data is point-in-time
- WS streams the full state snapshot every tick — no delta encoding; payload grows linearly with device count

**Config model**
- `plant.json` conflates **topology** (which devices are wired how) with **ops** concerns (`deploy_target`, `port`) — these change at different rates and for different reasons
- `DataType` enum is dual-purpose: it is both a schema marker (`DataType::Float`) and a value container (`DataType::Float(20.0)`) — confusing and prevents strong typing of schema vs values
- `device_types.json` physics scripts are plain strings inside JSON — no syntax highlighting, no linting, no import system; hard to maintain as scripts grow

**Frontend**
- 3D objects are hardcoded per device type (`boiler.ts`, `valve.ts`, `flow-meter.ts`) — adding a new device type requires new frontend code; no dynamic rendering from schema
- `device_types.json` already has everything needed to render a generic data panel — completely unused by the frontend
- No historical chart, trend view, or alarm panel — operators see only current values with no context

**Security**
- No auth on `/ws`, `/api/plant`, or `/api/log-level` — the log-level endpoint is a minor data-leak surface (can enable TRACE)
- OPC-UA endpoints accept anonymous connections — any host on the Tailscale network (including compromised nodes) can connect to a simulator

**Operational gaps**
- No metrics/observability on the platform itself (no Prometheus endpoint, no structured log shipping)
- No liveness/readiness probe on simulators — only backend has `/health`; k3s can't detect a deadlocked OPC-UA server thread
- `emptyDir` for PKI data — certs regenerate on every pod restart; peers reject the new cert until they re-trust, causing connector backoff storms

---

## What is redundant

- **`OPCUA_URI_OVERRIDE` env var** — introduced as a dev convenience but is a footgun that silently rewrites all PLC URLs; should be replaced by a proper local dev profile in plant.json or a `.env` override per PLC
- **`deploy_target` and `port` inside `plant.json`** — operational parameters that don't belong in the plant physics definition; they change when you move a PLC to a new node, not when the physics change; belong in `platform.json` or Helm values
- **codegen crate** — `gen_helm_values.rs` is ~60 lines of string templating; the `plant_config` parser dependency adds compile overhead for what is a config transformation task; a `jq` pipeline or small Python script would do the same with less ceremony
- **`protocol: "opcua"` field on PlcConfig** — only one protocol exists; the field creates a false sense of extensibility without delivering it; remove until a second protocol lands

---

## Core product vision: live factory editing ⚠️

> **This is the defining capability that separates this platform from a static dashboard.**

The current model requires a developer with repo access and a running CI pipeline to change anything about the plant — add a device, rewire two sensors, change a physics parameter. That is not how operators work.

The target model: an operator opens the frontend, drags a new flow meter onto the canvas, connects it to an existing valve, sets a flow coefficient, and hits **Run**. The simulation loop for that device starts within seconds, with no deployment, no JSON editing, no `just helm-deploy`.

**What this requires (and does not yet exist):**

- **Shared config DB** — the authoritative plant topology lives in a database (PostgreSQL or SQLite to start), not in `plant.json`. The backend owns the DB and exposes `GET/PUT /api/config` so any frontend or API client can read and modify the topology live.
- **Hot-reload in the simulator** — the simulator process (or a new coordinator process) watches the config DB for changes and can spin up/tear down the physics tick loop for individual devices without a pod restart. Today the entire process restarts to pick up any config change.
- **Frontend topology editor** — the 3D canvas becomes writable: drag devices, draw connections, set params, hit Run. The Three.js scene is already device-aware; it needs a write path back to the backend API.
- **Config bootstrap** — on first boot, the backend seeds the DB from `plant.json` (or a blank schema). After that, `plant.json` is a snapshot/export format, not the source of truth. The ConfigMap and `--set-file` pattern disappears.

**Why this matters beyond convenience:** Without live config, every new customer plant requires a developer to edit JSON files and redeploy. With live config, a non-technical operator can model their own factory, connect real PLCs or simulated ones, and start collecting data — the platform becomes self-service. This is the critical path to a product, not just a prototype.

**What is already in place:** `ResolvedPlant` is built once from config at startup — the architecture already separates "config reading" from "runtime state". The physics engine (Rhai scripts) is already hot-swappable in principle. The `GenericConnector` trait already handles one-connector-per-PLC. The gaps are the write path (no DB, no API mutations) and the hot-reload path (no dynamic device add/remove without restart).

---

## What is missing but would help

### Low effort, high immediate impact
- **Persistent PKI volume** — replace `emptyDir` with a PVC for `/data`; eliminates cert churn and trust failures on pod restarts; one line change in the Helm template
- **Health endpoints on simulators** — each simulator process could expose `/health` on a secondary port; enables k8s liveness probes; right now a deadlocked simulator looks healthy to k3s
- **Device function invocation API** — `FunctionKind` (`set_target_temperature`, etc.) is fully defined in the schema and Rhai engine but there is no HTTP endpoint; `POST /api/plcs/{plc}/devices/{id}/functions/{name}` would make the platform interactive
- **Telemetry persistence** — even SQLite with a `(ts, device_id, field, value REAL)` table in the backend would enable historical queries; QuestDB for production-scale ingestion

### Architecture-changing (medium term)
- **Config API + live topology DB** (`GET/PUT /api/config`) — the most important capability gap; see "Core product vision: live factory editing" above. Backend seeds a DB from `plant.json` on first boot, exposes config over REST, simulator hot-reloads on change. ConfigMap and `--set-file` disappear. Frontend gains a write path to add/remove/rewire devices without redeployment.
- **Delta WS updates** — diff previous snapshot and send only changed fields per tick; trivial to implement, essential once device count exceeds ~50; reduces frontend CPU significantly
- **Dynamic frontend device rendering** — generic `DeviceCard` component driven by `device_types.json` metrics list; new device types added in config only; eliminates the per-device-type `.ts` files
- **Alarm/threshold engine** — per-metric `min`/`max` bounds in `device_types.json`; backend evaluates on ingest and emits alarm events over WS; frontend shows alert overlay without polling

### Direction-changing (longer term)
- **MQTT PubSub alongside OPC-UA** — PLCs that support MQTT Sparkplug B skip the polling loop entirely; backend gains `MqttConnector` alongside `ScadaPlcConnector`; the `GenericConnector` trait already supports this pattern
- **Unified Namespace layer** — as plant count grows, a single MQTT broker as central data hub lets any consumer subscribe to any metric without routing through the backend; aligns with ISA-95 hierarchy already implied by `plant → plcs → devices`
- **Cross-PLC wiring** — today only intra-PLC device dependencies are resolved locally in the physics tick; cross-PLC wiring requires a coordinator (or a shared MQTT topic) — natural evolution once the MQTT layer exists
- **Replay / scenario engine** — persist physics snapshots, replay at any speed; useful for operator training and fault root-cause analysis without needing a live plant

---

## Technical direction suggestions

### Immediate (next sprint)
1. PVC for PKI — eliminates the most operationally disruptive bug (cert churn on restart)
2. Simulator `/health` endpoint — makes k3s restarts actually useful
3. `POST /api/devices/{id}/functions/{name}` — first interactive capability; turns this from a dashboard into a control interface

### Short term (1–2 months)
4. Telemetry persistence — SQLite in backend to start; swap to QuestDB when query volume justifies it
5. **Config DB + live topology API** — single most impactful architectural change; the entire "live factory editing" vision (see above) depends on this; unlocks self-service plant modelling without developer involvement
6. Delta WS encoding — reduces WS payload size significantly as plant grows
7. Split `plant.json` into topology (physics/wiring) and ops (ports/targets) — different owners, different change cadence

### Medium term (protocol evolution)
8. `rumqttc` MQTT connector — `GenericConnector` already accepts new impls; MQTT Sparkplug B for new devices; OPC-UA stays for legacy hardware
9. OPC-UA PubSub (Part 14 over MQTT) — migration path that keeps the OPC-UA data model but switches to MQTT transport; best of both worlds for mixed fleets
10. Generic frontend renderer — one `DevicePanel` component, N device types, zero frontend changes per new device type

### Longer term (platform maturity)
11. ISA-95 hierarchy in plant model (`plant → site → area → line → cell → device`) — aligns with UNS pattern; enables MES/ERP integration and multi-site deployments
12. Prometheus `/metrics` on backend + Grafana on k3s — observability on the platform itself, not just the simulated plant
13. JWT auth on API/WS — even a single static token blocks the most obvious abuse
14. **Upgrade OPC-UA client from `opcua 0.12` to async-native** ⚠️ — `open62541` Rust bindings are OPC Foundation–certified and actively maintained; alternatively wait for an async fork of the pure-Rust crate. The shared-runtime bug (see Protocol layer weakness above) makes this a must-fix before production, not a nice-to-have. Workaround (`drop(conn)` before reconnect) is in place but does not fix the underlying architecture.

### What to avoid
- Don't adopt EdgeX Foundry — it replaces most of what you've built; only viable if starting from scratch on a heterogeneous device fleet
- Don't migrate to WASM physics yet — Rhai is simpler and sufficient; WASM adds toolchain complexity with no current benefit
- Don't move to full Kubernetes (EKS/GKE) — k3s IS certified Kubernetes; no manifest changes needed if you ever do migrate; premature at current node count
- Don't abstract protocols before you have two concrete connectors — the `GenericConnector` trait is correct in place, but don't spend time on the abstraction layer until MQTT lands
