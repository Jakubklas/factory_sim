# Water Cleaning Plant — Digital Twin

## Overview

A real-time digital twin of a water cleaning plant. The backend simulates OPC-UA devices (boilers, pressure meters, flow meters, valves) using Rust. The frontend renders a dark-themed 3D visualization of the plant floor in the browser using TypeScript and Three.js, with live data streaming over WebSockets.

---

## Stack

| Layer | Technology | Purpose |
|-------|-----------|---------|
| Simulator | Rust + `opcua` crate | OPC-UA server simulating plant devices |
| API/Bridge | Rust + `axum` + `tokio` | WebSocket server bridging OPC-UA data to the browser |
| Frontend | TypeScript + Three.js | 3D visualization with live data overlays |
| Build tool | Vite | TypeScript compilation, dev server, hot reload |

---

## Project Structure

```
water-plant-twin/
├── backend/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs              # Entry point — starts OPC-UA server + Axum WS server
│       ├── simulator/
│       │   ├── mod.rs            # Re-exports simulator modules
│       │   ├── plant.rs          # Plant struct — owns all devices, runs simulation loop
│       │   ├── devices.rs        # Device structs: Boiler, PressureMeter, FlowMeter, Valve
│       │   └── physics.rs        # Simple physics: temperature curves, pressure propagation, flow rates
│       ├── opcua_server/
│       │   ├── mod.rs
│       │   └── server.rs         # OPC-UA server setup, node creation, subscription handling
│       └── ws_bridge/
│           ├── mod.rs
│           └── bridge.rs         # Axum WebSocket endpoint, reads OPC-UA state, fans out to clients
│
├── frontend/
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   ├── index.html
│   └── src/
│       ├── main.ts               # Entry point — initializes scene, connects WebSocket
│       ├── scene/
│       │   ├── setup.ts          # Three.js scene, camera, renderer, lighting, OrbitControls
│       │   ├── factory-floor.ts  # Ground plane, grid, walls — the environment
│       │   └── post-processing.ts # Bloom, ambient occlusion for dark industrial look
│       ├── objects/
│       │   ├── boiler.ts         # Boiler 3D geometry + material
│       │   ├── pressure-meter.ts # Pressure meter geometry
│       │   ├── flow-meter.ts     # Flow meter geometry
│       │   ├── valve.ts          # Valve geometry
│       │   └── pipe.ts           # Pipe geometry + animated flow particles
│       ├── overlays/
│       │   ├── label.ts          # CSS2DRenderer label factory — creates hovering data boxes
│       │   └── styles.css        # Dark glassmorphism styling for overlay labels
│       └── data/
│           ├── websocket.ts      # WebSocket client — connects to backend, parses messages
│           └── state.ts          # Plant state store — holds latest values, notifies 3D objects
│
├── ARCHITECTURE.md
└── README.md
```

---

## Backend Detail

### Devices and Simulation

Each device is a Rust struct that holds its current state and updates on a tick cycle (e.g. 100ms).

```
Boiler
├── id: String
├── temperature: f64          # °C, simulated with heat curve
├── target_temperature: f64   # setpoint
├── pressure: f64             # bar, derived from temperature
├── status: enum (Off, Heating, Steady, Overheat)

PressureMeter
├── id: String
├── pressure: f64             # bar, reads from connected pipe/boiler
├── status: enum (Normal, Warning, Critical)

FlowMeter
├── id: String
├── flow_rate: f64            # liters/min
├── total_volume: f64         # cumulative liters
├── status: enum (Normal, Low, High)

Valve
├── id: String
├── position: f64             # 0.0 (closed) to 1.0 (fully open)
├── mode: enum (Manual, Auto)
├── status: enum (Open, Closed, Partial, Fault)
```

### Plant Topology

The plant has a simple linear topology for the initial build:

```
[Boiler 1] → Pipe → [Pressure Meter 1] → Pipe → [Valve 1] → Pipe → [Flow Meter 1] → Pipe → [Boiler 2]
                                                       ↓
                                              [Drain / Output]
```

- Boiler 1 heats water and builds pressure
- Pressure Meter 1 reads the output pressure
- Valve 1 controls flow downstream (auto-regulates based on pressure)
- Flow Meter 1 measures throughput
- Boiler 2 receives the flow for secondary treatment
- Valve 1 can also divert to drain when pressure exceeds safe limits

### Simulation Physics (Keep Simple)

- Boiler temperature: ramp toward target at a rate, with small noise. Pressure = f(temperature) using a basic linear approximation
- Flow rate: proportional to upstream pressure × valve position
- Pressure propagation: downstream pressure decays from upstream based on pipe length and flow
- Add small random noise (±1-3%) to all readings for realism
- Occasional fault injection: a valve sticks, a boiler overshoots, a flow meter spikes

### OPC-UA Server

- One OPC-UA server instance with a single namespace
- Each device maps to an OPC-UA object node with variable child nodes for each property
- Node IDs follow the pattern: `ns=1;s=<DeviceType>.<DeviceId>.<Property>` (e.g. `ns=1;s=Boiler.boiler-1.temperature`)
- Update interval: 100ms for simulation tick, OPC-UA publish interval: 500ms
- Use `opcua` crate's server builder API

### WebSocket Bridge

- Axum route: `GET /ws` — upgrades to WebSocket
- On connection: send full plant state snapshot (all devices, all properties)
- On each OPC-UA publish cycle: send only changed values as a delta update
- Message format (JSON):

```json
{
  "type": "snapshot" | "delta",
  "timestamp": "2025-01-01T00:00:00Z",
  "devices": {
    "boiler-1": {
      "temperature": 82.3,
      "target_temperature": 85.0,
      "pressure": 3.2,
      "status": "Heating"
    },
    "valve-1": {
      "position": 0.73,
      "mode": "Auto",
      "status": "Partial"
    }
  }
}
```

- Support multiple simultaneous browser clients via broadcast channel

---

## Frontend Detail

### Scene Setup

- Renderer: `WebGLRenderer` with `antialias: true`, dark background (`#0a0a0f`)
- Camera: `PerspectiveCamera`, positioned to see the full plant floor
- Controls: `OrbitControls` with damping enabled, constrained to prevent flipping below ground
- Lighting: dim ambient light + a few point lights with warm tones near boilers, cool tones near pipes
- Ground: large plane with a subtle grid texture
- Post-processing (phase 2): `EffectComposer` with `UnrealBloomPass` for glowing elements

### 3D Objects

All objects are built from basic Three.js geometries. No imported models.

```
Boiler       → CylinderGeometry (large, vertical), metallic dark gray material, 
               emissive glow that shifts from blue (cold) to orange (hot) based on temperature
               
Pressure     → Small SphereGeometry on a BoxGeometry stand, with a ring (TorusGeometry)
Meter          Color shifts: green (normal) → yellow (warning) → red (critical)

Flow Meter   → BoxGeometry (small inline device), with animated internal particles
               showing flow direction and speed

Valve        → Two CylinderGeometry flanges connected by a smaller cylinder
               Rotates/animates to show open/closed position

Pipe         → TubeGeometry along a path between devices
               Animated small spheres (instanced) travel along the pipe to show flow
               Speed and density of particles = flow rate
               Color: dark blue-gray base, particles are bright cyan/blue
```

### Hovering Labels (CSS2D Overlays)

Each device gets a floating HTML label positioned above it in 3D space using `CSS2DRenderer`.

Label content:
```
┌─────────────────────┐
│ BOILER-1             │
│ 82.3°C → 85.0°C     │
│ 3.2 bar              │
│ ● Heating            │
└─────────────────────┘
```

Styling:
- Dark semi-transparent background (`rgba(10, 10, 20, 0.85)`)
- Thin border with color matching device status (green/yellow/red)
- Monospace font, small text
- Subtle backdrop blur if supported

### WebSocket Client

- Connect to `ws://localhost:3000/ws`
- On `snapshot`: initialize full state store, create/update all 3D objects and labels
- On `delta`: update only changed values in state store, update affected labels and visuals
- Auto-reconnect with exponential backoff on disconnect

### State Store

A simple TypeScript object (no library) that holds the latest state for every device. When a value updates, it triggers callbacks registered by the 3D objects and labels.

```typescript
interface PlantState {
  devices: Record<string, DeviceState>;
  subscribe(deviceId: string, callback: (state: DeviceState) => void): void;
  update(delta: DeltaMessage): void;
}
```

---

## Build and Run

### Backend
```bash
cd backend
cargo run
# Starts OPC-UA server on opc.tcp://localhost:4840
# Starts WebSocket server on ws://localhost:3000/ws
```

### Frontend
```bash
cd frontend
npm install
npm run dev
# Starts Vite dev server on http://localhost:5173
# Proxied WebSocket requests to localhost:3000
```

---

## Phase Plan

### Phase 1 — Skeleton (Build This First)
- [ ] Rust: Device structs with hardcoded initial values, simulation loop ticking every 100ms
- [ ] Rust: Axum WebSocket endpoint sending full state as JSON on each tick
- [ ] TS: Three.js scene with ground plane, camera, OrbitControls
- [ ] TS: Placeholder box geometries for each device, positioned along the plant topology
- [ ] TS: WebSocket client connecting and logging messages to console
- [ ] TS: Basic CSS2D labels showing device ID above each object

### Phase 2 — Live Data
- [ ] Rust: Simulation physics — temperature ramps, pressure derivation, flow calculation
- [ ] Rust: Delta updates instead of full snapshots on every tick
- [ ] TS: State store with subscriptions
- [ ] TS: Labels update with live values
- [ ] TS: Object colors/emissive change based on state (hot boiler glows orange, etc.)

### Phase 3 — Visual Polish
- [ ] TS: Pipe particle animations showing flow direction and speed
- [ ] TS: Valve rotation animation reflecting position
- [ ] TS: Post-processing (bloom pass for glowing elements)
- [ ] TS: Status-colored label borders
- [ ] Rust: Fault injection — occasional random device faults

### Phase 4 — Interactivity
- [ ] TS: Click on a device to select it, show detailed panel
- [ ] TS: Manual valve control from the UI (sends command back over WebSocket)
- [ ] Rust: Accept commands from frontend via WebSocket (e.g. set valve position, change boiler target)
- [ ] TS: Historical mini-charts in device panels (last 60 seconds of readings)

---

## Conventions

- Rust: use `snake_case`, group related types in modules, derive `Serialize`/`Deserialize` on all state types
- TypeScript: use `camelCase` for variables/functions, `PascalCase` for types/interfaces
- Keep files small — one device type per file, one concern per module
- Comments: explain *why*, not *what* — the code should be readable on its own
- No premature optimization — get it working, then profile if needed