# Data model

Postgres is the single source of truth for everything the user builds. At runtime the
backend reads only the DB; the seed JSON is consulted once and never again. Schema lives
in [`backend/migrations/`](../backend/migrations/) (sqlx migrations, applied at startup).

> Interactive diagram: [`docs/diagrams/db-schema.excalidraw`](diagrams/db-schema.excalidraw).

---

## Tables

```
  deploy_nodes                      cluster machines that can host a sim PLC
    name PK · display_name · k8s_node · arch · mem_allocatable_mb · status · last_seen
        ▲  (synced live from the k8s API by the node-sync loop)
        │ deploy_node (FK, SET NULL)
  plcs                              a PLC: simulated (→ deploy_node) or real (→ endpoint_uri)
    id PK · plc_id(slug) · name · kind('simulated'|'real') · deploy_node · endpoint_uri
          · port · protocol · endpoint
        ▲                                   ▲
        │ plc_id (FK, CASCADE)              │ device_type_id (FK)
  device_instances                  a device_type placed on a PLC, with params
    id PK · plc_id · device_type_id · device_id(slug) · name · param_values(jsonb) · floor_pos
        ▲                                   │
        │ dst_instance_id (FK, CASCADE)     │
  wires                             upstream output → downstream input port
    id PK · src_plc_id · src_device_id · src_field · dst_instance_id · dst_input_port

  device_types     id PK · name · physics_rhai · io_spec(jsonb) · model_ref · icon_ref
  discovered_nodes (plc_id,node_id) PK · browse_path · browse_name · datatype · access_level
  audit_log        id PK · entity · entity_id · before(jsonb) · after(jsonb) · at
```

Normalized tables with FKs for reused/wired entities; **JSONB** for the genuinely
schemaless parts (`param_values`, `floor_pos`, and `io_spec`).

| Table | Holds | Notes |
|---|---|---|
| `device_types` | Physics + I/O contract per type. | `io_spec` stores the **full `DeviceTypeDefinition`** JSON so it round-trips back into a Rust struct. |
| `plcs` | Sim or real PLC. | `plc_id` slug doubles as the k8s deploy name and OPC-UA hostname. |
| `device_instances` | A type placed on a PLC. | `device_id` slug appears in OPC-UA node paths and `IngestedState` keys. |
| `wires` | Any output → any input. | `src_device_id`+`src_field` resolve the value; `dst_input_port` + the dest device's path build the write node id. Cross-PLC allowed. |
| `deploy_nodes` | Cluster nodes. | Kept truthful by node-sync; the PLC builder only offers Ready nodes. |
| `discovered_nodes` | Cached browse results. | The live cache is in-memory (`DiscoveredState`); this table is the persisted form. |
| `audit_log` | Before/after per change. | For history / rollback. |

---

## DB ↔ runtime round-trip

The DB stores normalized rows; the runtime wants a `PlantConfig` tree and a flat wire table.
[`db/plant_loader.rs`](../backend/src/db/plant_loader.rs) bridges them:

```
  load(pool)              → (PlantConfig, [DeviceTypeDefinition])   full plant at startup
  load_wires(pool)        → [OrchestratorWire]                      flat list for the tick
  load_plc_config(pool,id)→ (PlcConfig, [DeviceTypeDefinition])     one PLC, for sim self-build
```

`load` joins `plcs ⋈ device_instances ⋈ device_types ⋈ wires`, rebuilding each device's
`input_variables` from wire rows. `io_spec` is deserialized straight back into
`DeviceTypeDefinition`. `load_wires` pre-computes each wire's destination node id
(`ns=2;s={plc}.{device}.{input}`) so the orchestration tick does no string work.

---

## NOTIFY triggers

Live editing without restarts is built on Postgres `LISTEN/NOTIFY`
([`0002`](../backend/migrations/0002_schema_additions.sql),
[`0003`](../backend/migrations/0003_runtime_fixes.sql)):

```
  INSERT/UPDATE/DELETE on  plcs · device_instances · wires
        └─► notify_plant_changed()  →  pg_notify('plant_changed', '<op>:<table>')
                                          └─► backend LISTEN loop reacts
```

`0003` also adds a row-level trigger `cleanup_wires_for_deleted_instance`: deleting a device
instance removes wires that named it as a source (`src_device_id` is a plain slug, not an FK),
preventing orphaned wires from breaking a later plant load.

---

## Migrations

| File | Adds |
|---|---|
| [`0001_initial_schema.sql`](../backend/migrations/0001_initial_schema.sql) | All tables, the `plc_kind` enum, indexes. |
| [`0002_schema_additions.sql`](../backend/migrations/0002_schema_additions.sql) | `plc_id`/`protocol`/`endpoint` on `plcs`; `device_id` on instances; typed `src_device_id`/`src_field` on wires (replacing `src_node`); the `plcs` NOTIFY trigger. |
| [`0003_runtime_fixes.sql`](../backend/migrations/0003_runtime_fixes.sql) | Drops the over-strict `plc_kind_check`; extends NOTIFY to instances+wires; orphan-wire cleanup trigger. |

---

## Seeding

[`db/seed.rs`](../backend/src/db/seed.rs). Runs **only when `SEED_DIR` is set and the DB is
empty**. Imports `device_types.json`, `plant.json`, and (optionally) `inventory.json` from
one directory, then stops. After that the JSON files are never read again — the DB is canonical.
This is the one-time bridge from the old file-based config to the live database.

Order matters and is fixed: **`deploy_nodes` → `device_types` → `plcs` + `device_instances` → `wires`**
(each step satisfies the next step's foreign keys). Each device's `input_variables` are turned
into `wires` rows in the final pass — after every instance exists, so cross-PLC sources resolve.
