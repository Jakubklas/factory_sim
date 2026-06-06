# Frontend

TypeScript · Vite · Three.js. A read-only 3D viewer of the running plant. It talks only to
the backend — `GET /api/plant` to learn the scene, `/ws` for live state — and renders no
controls (it does not write setpoints or edit the plant). Source: [`frontend/src/`](../frontend/src/).

```
   fetchPlantConfig()  GET /api/plant  ─► build scene once (devices, pipes, PLC nodes)
                                          │
   connectToBackend()  ws://…/ws  ─► WsClient ─► state.ts ─► subscribe(deviceId, update)
                                                              └─► per-device 3D + label update
```

---

## Layout

| Path | Role |
|---|---|
| [`main.ts`](../frontend/src/main.ts) | Bootstraps the scene: fetch config, place devices along X, link them with pipes, hover a PLC node over each PLC's devices, subscribe each device to live state. |
| [`data/plant-client.ts`](../frontend/src/data/plant-client.ts) | `fetchPlantConfig()` — the `/api/plant` call. |
| [`data/ws-client.ts`](../frontend/src/data/ws-client.ts) | Pure WebSocket transport: connect, exponential-backoff reconnect, clean shutdown. Knows nothing about plant data. |
| [`data/state.ts`](../frontend/src/data/state.ts) | Parses each frame, dispatches `device_id → fields` to subscribers. |
| [`data/types.ts`](../frontend/src/data/types.ts) | `DeviceConfig`, `FieldValues`, etc. |
| [`objects/`](../frontend/src/objects/) | Per-device-type 3D builders + updaters: `boiler`, `valve`, `flow-meter`, `pipe`, `plc-node`, `pressure-meter`. |
| [`scene/`](../frontend/src/scene/) | `setup` (camera/renderer/controls), `factory-floor`, `post-processing`. |
| [`overlays/`](../frontend/src/overlays/) | CSS2D labels rendered over the 3D scene. |

---

## How a device renders

`buildEntry()` in [`main.ts`](../frontend/src/main.ts) switches on `device.device_type`
(`Boiler`, `Valve`/`CyclicalValve`, `FlowMeter`) to build the 3D group + label and returns an
`update(fields)` closure. Each entry then `subscribe()`s to its `device_id`; every WS frame
calls the matching closure, which maps fields → geometry/material/label. Unknown device types
are logged and skipped, so the scene degrades gracefully as new types are added server-side.

The scene is **config-driven** — device IDs, names, and PLC grouping all come from
`/api/plant`, nothing is hardcoded. Add a device in the DB and it appears on next load.

---

## Running & serving

```bash
cd frontend && npm install && npm run dev      # Vite dev server
```

The WebSocket URL is resolved at runtime ([`config.ts`](../frontend/src/config.ts) +
`public/env.js`), so the same built image points at different backends per environment. In the
cluster the frontend is an nginx container ([`frontend/docker/`](../frontend/docker/)) behind
Tailscale Funnel — see [deployment.md](deployment.md).
