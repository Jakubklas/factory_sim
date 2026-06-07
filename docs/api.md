# API reference

Served by the backend ([`api/ws_bridge.rs`](../backend/src/api/ws_bridge.rs)) on
`BE_HOST:BE_PORT` (default `0.0.0.0:3001`). CORS is permissive. All `/api/*` data routes require a database;
without `DATABASE_URL` they return `503`. The frontend uses only `/api/plant` and `/ws`.

Status conventions: `200`/`201` success, `204` on delete, `400` malformed input (e.g. a
non-UUID path), `404` missing, `409` constraint conflict (e.g. deleting a device type still
in use), `503` no database. For instance routes the URL `:plc_id` is authoritative — a
request can only create or delete instances on the PLC named in the path.

---

## Real-time & meta

| Method · Path | Purpose | Returns |
|---|---|---|
| `GET /ws` | Live telemetry stream. Pushes the full `IngestedState` as JSON every tick. | WebSocket |
| `GET /api/plant` | Current `PlantConfig` (PLC + device tree). Drives the frontend scene. | `PlantConfig` |
| `GET /health` | Liveness. | `{"status":"ok"}` |
| `GET /api/log-level?set=<filter>` | Live `RUST_LOG` reload (e.g. `?set=debug`). No `set` → usage. | text |

WebSocket frame shape (`device_id → field → value`):

```json
{ "boiler_001": { "temperature": 84.2, "pressure": 3.0, "status": "heating" },
  "valve_001":  { "position": 0.41, "outlet_pressure": 2.8 } }
```

---

## Libraries & topology (CRUD)

| Method · Path | Purpose |
|---|---|
| `GET / POST /api/device-types` · `DELETE /api/device-types/:name` | Device-type library (keyed by `name`). |
| `GET / POST /api/plcs` · `DELETE /api/plcs/:id` | PLCs (sim or real). |
| `GET / POST /api/plcs/:plc_id/instances` · `DELETE /api/plcs/:plc_id/instances/:id` | Device instances on a PLC. |
| `GET / POST /api/wires` · `DELETE /api/wires/:id` | Wires (any output → any input). |
| `GET /api/deploy-nodes` | Cluster nodes available to host sim PLCs. |

`POST` bodies are the `Create*` DTOs in [`db/models.rs`](../backend/src/db/models.rs).
Slugs (`plc_id`, `device_id`) auto-derive from `name` if omitted. Examples:

```jsonc
// POST /api/plcs
{ "name": "Bypass PLC", "kind": "simulated", "deploy_node": "pi" }

// POST /api/plcs/:plc_id/instances
{ "plc_id": "<uuid>", "device_type_name": "Boiler", "name": "Main Boiler",
  "param_values": { "ramp_rate": 2.0, "max": 150.0 } }

// POST /api/wires   — boiler_001.pressure → this instance's "inlet_pressure" input
{ "src_plc_id": "<uuid>", "src_device_id": "boiler_001", "src_field": "pressure",
  "dst_instance_id": "<uuid>", "dst_input_port": "inlet_pressure" }
```

Any successful write fires a NOTIFY; the backend picks up the change within a tick / one
reconcile interval — no restart. See [data-model.md › NOTIFY triggers](data-model.md#notify-triggers).

---

## Runtime control

| Method · Path | Purpose |
|---|---|
| `GET /api/plcs/:plc_id/discovered` | A PLC's live browsed node tree (`[BrowsedNode]`), from the in-memory browse cache. `:plc_id` is the UUID. |
| `POST /api/setpoint` | Write a value to a writable node (sim or real) — the same write path wires use. |
| `GET /api/plcs/:plc_id/config` | **Simulator self-build** — a sim pod fetches `{ plc, device_types }` to construct itself. Rejected for real PLCs. `:plc_id` may be a UUID or slug. |

```jsonc
// POST /api/setpoint   → 202 Accepted (enqueued; dropped if the connector queue is full)
{ "plc_name": "Heating PLC", "node_id": "ns=2;s=Heating PLC.boiler_001.target_temperature",
  "value": 90.0 }                       // value: number | bool | string
```

> ⚠ Setpoint writes can drive **real** hardware. Range-check and confirm before writing to
> a real PLC — the backend forwards the write verbatim.
