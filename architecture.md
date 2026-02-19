# Factory Sim — Architecture

## Overview

A **water plant digital twin** built in Rust. Devices are fully defined in JSON — no Rust code
changes needed to add new device types, reconfigure the plant, or change physics. Inspired by
Apache Flink's stream-processing model: devices are operators (Source → Transform → Sink) that
consume inputs, apply physics, and produce outputs.

---

## Repository Layout

```
factory_sim/
├── backend/
│   ├── src/
│   │   ├── main.rs                      # Entry point, task orchestration
│   │   ├── simulator/
│   │   │   ├── plant.rs                 # Plant (simulation graph)
│   │   │   ├── devices.rs               # Device, InputPort, OutputPort
│   │   │   ├── physics.rs               # Pure physics utility functions
│   │   │   ├── physics_functions.rs     # Trait + implementations + factory
│   │   │   └── device_functions.rs      # Trait + implementations + factory
│   │   ├── models/
│   │   │   ├── devices/
│   │   │   │   ├── device_config.rs     # DeviceConfig, DeviceConfigRegistry, Topology
│   │   │   │   ├── device_schema.rs     # DeviceSchema, DeviceSchemaRegistry
│   │   │   │   ├── device_handle.rs     # DeviceHandle, DeviceFieldValue
│   │   │   │   ├── device_plc_mapping.rs # DeviceMapping
│   │   │   │   └── metrics/
│   │   │   │       └── metric_config.rs # MetricConfig, DataType
│   │   │   ├── plc/
│   │   │   │   └── plc_config.rs        # PlcConfig
│   │   │   └── plant/
│   │   │       └── plant_config.rs      # PlantConfig
│   │   ├── opcua_server/
│   │   │   ├── plc_server.rs            # OPC UA server per PLC
│   │   │   └── scada_client.rs          # SCADA: reads all PLCs, broadcasts state
│   │   └── ws_bridge/
│   │       └── bridge.rs                # WebSocket server (Axum, port 3000)
│   └── config/
│       ├── available_devices.json       # Device type schemas
│       ├── devices.json                 # Device instances (physics, ports, functions)
│       ├── topology.json                # Execution order
│       └── factory.json                 # PLC servers + OPC UA node mappings
└── frontend/                            # Vite/TypeScript UI (npm run dev → :5173)
```

---

## Models Hierarchy

```
models/
│
├── DeviceFieldValue                     # The atomic value type used everywhere
│   ├── Float(f64)
│   └── String(String)
│
├── ── Device Schema (what types exist) ──────────────────────────────────────
│
├── DataType                             # OPC UA type tag
│   ├── Double
│   └── String
│
├── DeviceFieldSchema                    # One field of a device type
│   ├── name: String
│   ├── data_type: DataType
│   └── description: String
│
├── DeviceSchema                         # All fields for a device type
│   ├── device_type: String              # e.g. "Boiler"
│   └── fields: Vec<DeviceFieldSchema>
│
└── DeviceSchemaRegistry                 # Loaded from available_devices.json
    └── device_schemas: Vec<DeviceSchema>
        source: DeviceSchemaRegistry::from_json("config/available_devices.json")


├── ── Device Configuration (instances) ───────────────────────────────────────
│
├── DeviceCategory
│   ├── Source                           # No inputs → N outputs (boiler, sensor)
│   ├── Transform                        # N inputs → M outputs (valve, mixer)
│   └── Sink                             # N inputs → 0 outputs (display, logger)
│
├── InputPort                            # Where this device reads an input from
│   ├── name: String                     # e.g. "upstream_pressure"
│   ├── source_device_id: String         # e.g. "boiler-1"
│   └── source_field: String             # e.g. "pressure"
│
├── OutputPort                           # What this device produces
│   ├── name: String                     # e.g. "temperature"
│   └── target_field: String             # internal device field name
│
├── PhysicsFunctionConfig (tag: "type")  # Physics model, JSON-selectable
│   ├── TemperatureRamp { ramp_rate, min, max, pressure_from_temp }
│   ├── PressurePassthrough { noise_percent }
│   ├── FlowCalculation { coefficient, accumulate_volume }
│   ├── PressureRegulator { target_pressure, kp }
│   └── Static                           # No automatic updates
│
├── DeviceFunctionConfig (tag: "type")   # Control function, JSON-selectable
│   ├── SetField { field, value }
│   ├── SetFieldFromArg { field, arg_index }
│   └── IncrementField { field, amount }
│
├── FunctionConfig                       # Named wrapper for DeviceFunctionConfig
│   ├── name: String                     # e.g. "open"
│   └── config: DeviceFunctionConfig     # (flattened in JSON)
│
├── DeviceConfig                         # One device instance
│   ├── id: String
│   ├── device_type: String
│   ├── category: DeviceCategory
│   ├── input_ports: Vec<InputPort>
│   ├── output_ports: Vec<OutputPort>
│   ├── physics_function: PhysicsFunctionConfig
│   ├── functions: Vec<FunctionConfig>
│   └── initial_values: HashMap<String, serde_json::Value>
│
├── DeviceConfigRegistry                 # Loaded from devices.json
│   └── devices: Vec<DeviceConfig>
│       source: DeviceConfigRegistry::from_json("config/devices.json")
│
└── Topology                             # Loaded from topology.json
    └── execution_order: Vec<String>     # device IDs in dependency order
        source: Topology::from_json("config/topology.json")


├── ── PLC / OPC UA Configuration ─────────────────────────────────────────────
│
├── MetricConfig                         # One OPC UA node exposed per field
│   ├── node_path: String                # e.g. "Boiler1.Temperature"
│   ├── field_name: String               # e.g. "temperature"
│   └── data_type: DataType
│
├── DeviceMapping                        # Which fields of a device a PLC exposes
│   ├── device_id: String
│   ├── device_type: String
│   ├── folder_name: String              # OPC UA folder e.g. "Boiler1"
│   └── metrics: Vec<MetricConfig>
│
├── PlcConfig                            # One OPC UA server
│   ├── name: String                     # e.g. "PLC-1"
│   ├── uri: String                      # e.g. "urn:PLC1"
│   ├── port: u16                        # e.g. 4840
│   ├── endpoint: String                 # e.g. "opc.tcp://0.0.0.0:4840"
│   └── device_mappings: Vec<DeviceMapping>
│
└── PlantConfig                          # Top-level factory config
    ├── name: String                     # e.g. "water-plant"
    └── plcs: Vec<PlcConfig>
        source: PlantConfig::from_json("config/factory.json")


└── ── Runtime / Simulation Objects ───────────────────────────────────────────

    ├── PhysicsFunction (trait)          # Computes device outputs each tick
    │   fn compute(&self, device, inputs, dt) -> HashMap<String, DeviceFieldValue>
    │   Implementations:
    │     TemperatureRampPhysics
    │     PassthroughPhysics
    │     FlowCalculationPhysics
    │     PressureRegulatorPhysics
    │     StaticPhysics
    │
    ├── DeviceFunction (trait)           # Callable control action
    │   fn execute(&self, device, args) -> Result<(), String>
    │   Implementations:
    │     SetFieldFunction
    │     SetFieldFromArgFunction
    │     IncrementFieldFunction
    │
    ├── Device                           # Live simulation object
    │   ├── id, device_type, category
    │   ├── fields: HashMap<String, DeviceFieldValue>   # current state
    │   ├── input_ports: Vec<InputPort>
    │   ├── output_ports: Vec<OutputPort>
    │   ├── physics_function: Arc<dyn PhysicsFunction>
    │   └── functions: HashMap<String, Arc<dyn DeviceFunction>>
    │
    ├── Plant                            # Simulation graph
    │   ├── devices: HashMap<String, Device>
    │   └── execution_order: Vec<String>
    │
    ├── PlantState                       # Serializable snapshot (sent over WS)
    │   └── devices: HashMap<String, HashMap<String, DeviceFieldValue>>
    │
    └── DeviceHandle                     # Thread-safe access to a Device
        └── device: Arc<RwLock<Device>>  # shared across tokio + std threads
```

---

## Codebase Architecture

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         CONFIGURATION LAYER (JSON)                           │
│                                                                              │
│  available_devices.json   devices.json    topology.json    factory.json      │
│  (device type schemas)    (instances)     (exec order)     (PLC mappings)    │
└────────────┬──────────────────┬───────────────┬───────────────┬─────────────┘
             │                  │               │               │
             ▼                  ▼               ▼               ▼
      DeviceSchema        DeviceConfig      Topology       PlantConfig
      Registry            Registry                         (PlcConfigs)
             │                  │               │
             └──────────────────▼───────────────┘
                                │
                     Plant::from_config()
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                          SIMULATOR LAYER                                     │
│                                                                              │
│   Plant                                                                      │
│   ├── devices: HashMap<id, Device>                                           │
│   └── execution_order: [boiler-1, boiler-2, valve-1, ...]                   │
│                                                                              │
│   Device::tick(inputs, dt)                                                   │
│   ┌─────────────────────────────────────────────────────────────┐           │
│   │  inputs (from upstream via InputPorts)                       │           │
│   │      ↓                                                       │           │
│   │  PhysicsFunction::compute(device, inputs, dt)                │           │
│   │  ┌──────────────────────────────────────────────────────┐   │           │
│   │  │  TemperatureRamp │ Passthrough │ FlowCalc │ PIDCtrl  │   │           │
│   │  │         physics.rs: temperature_to_pressure()         │   │           │
│   │  │                     calculate_flow_rate()              │   │           │
│   │  │                     add_noise()                        │   │           │
│   │  └──────────────────────────────────────────────────────┘   │           │
│   │      ↓ outputs                                               │           │
│   │  device.fields updated via OutputPorts                       │           │
│   └─────────────────────────────────────────────────────────────┘           │
└────────────────────────────────────┬─────────────────────────────────────────┘
                                     │
                              Arc<RwLock<Plant>>
                                     │
              ┌──────────────────────┼──────────────────────┐
              │                      │                       │
      ┌───────▼───────┐     ┌────────▼────────┐    ┌────────▼────────┐
      │  Simulation   │     │  Device Sync    │    │  Heartbeat      │
      │  Loop (100ms) │     │  Task (50ms)    │    │  Logger (5s)    │
      │  plant.tick() │     │  plant→handles  │    │  log all fields │
      └───────────────┘     └────────┬────────┘    └─────────────────┘
                                     │
                         HashMap<id, DeviceHandle>
                         (Arc<RwLock<Device>> per device)
                                     │
              ┌──────────────────────┼──────────────────────┐
              │                      │                       │
              ▼                      ▼                       │
┌─────────────────────────────────────────────┐             │
│          OPC UA LAYER (std::thread)          │             │
│                                              │             │
│   PLC-1 (port 4840)     PLC-2 (port 4841)   │             │
│   ┌──────────────┐      ┌──────────────┐    │             │
│   │ Address Space │      │ Address Space │   │             │
│   │ ns=2;s=PLC-1. │      │ ns=2;s=PLC-2. │  │             │
│   │  Boiler1.Temp │      │  Boiler2.Temp │  │             │
│   │  Boiler1.Press│      │  Boiler2.Press│  │             │
│   │  Valve1.Pos   │      │  FlowMeter1.  │  │             │
│   │  ...          │      │  FlowRate ... │  │             │
│   └──────┬────────┘      └──────┬────────┘  │             │
│    update│100ms           update│100ms       │             │
│   DeviceHandle.read_field()    DeviceHandle.read_field()  │
└──────────┼────────────────────┼─────────────┘             │
           │  OPC UA protocol   │                            │
           ▼                    ▼                            │
┌──────────────────────────────────────────────┐            │
│         SCADA CLIENT (std::thread)            │            │
│                                               │            │
│   session.read("ns=2;s=PLC-1.Boiler1.Temp") │            │
│   session.read("ns=2;s=PLC-2.Boiler2.Press") │            │
│   → aggregates into PlantState               │            │
│   → tx.send(PlantState) [broadcast channel]  │◄───────────┘
└─────────────────────────┬────────────────────┘
                          │
                broadcast::channel<PlantState>
                          │
              ┌───────────▼────────────┐
              │   WebSocket Server     │
              │   Axum, port 3000      │
              │   GET /ws              │
              │                        │
              │   rx.recv() →          │
              │   JSON snapshot →      │
              │   socket.send()        │
              └───────────┬────────────┘
                          │
              ┌───────────▼────────────┐
              │    Browser Clients     │
              │    ws://localhost:3000 │
              │    /ws                 │
              │                        │
              │  { type: "snapshot",   │
              │    devices: {          │
              │      "boiler-1": {     │
              │        temperature: 85,│
              │        pressure: 4.3   │
              │      }, ...            │
              │    }                   │
              │  }                     │
              └────────────────────────┘
```

---

## Data Flow — Full Lifecycle

```
                    JSON Config Files
                          │
                          │ startup, once
                          ▼
                     Plant::from_config()
                     ┌───────────────────────────────────────────┐
                     │  For each DeviceConfig:                    │
                     │    1. Look up DeviceSchema (field defs)    │
                     │    2. Initialize fields from initial_values│
                     │    3. Create PhysicsFunction from config   │
                     │    4. Create DeviceFunction for each func  │
                     │    5. Store Device in Plant.devices        │
                     └───────────────────────────────────────────┘
                          │
                          ▼ Plant ready
                   ─────────────────────────────────────────────────
                   Simulation loop every 100ms (dt = 0.1s)
                   ─────────────────────────────────────────────────
                   For each device_id in execution_order:

                       GATHER INPUTS (via InputPorts)
                       ┌──────────────────────────────┐
                       │ upstream_pressure ←           │
                       │   boiler-1.pressure          │
                       │ valve_position ←              │
                       │   valve-1.position           │
                       └──────────────────────────────┘
                                    │
                       TICK (physics)
                       ┌──────────────────────────────┐
                       │ outputs = physics_fn.compute( │
                       │   device, inputs, dt=0.1)     │
                       │                               │
                       │ Boiler: temp += ramp_rate×dt  │
                       │ Valve:  pos += kp×(P_target   │
                       │              - P_upstream)    │
                       │ FlowM:  flow = f(press, pos)  │
                       │         volume += flow×dt     │
                       └──────────────────────────────┘
                                    │
                       UPDATE FIELDS (via OutputPorts)
                       ┌──────────────────────────────┐
                       │ device.fields["temperature"]  │
                       │   = outputs["temperature"]    │
                       │ device.fields["pressure"]     │
                       │   = outputs["pressure"]       │
                       └──────────────────────────────┘
                   ─────────────────────────────────────────────────
                          │
                          │ every 50ms
                          ▼
                   Device Sync Task
                   plant.get_device(id).clone()
                        → DeviceHandle.device.write() = clone
                          │
                          │ every 100ms
                          ▼
                   PLC Update Task
                   DeviceHandle.read_field("temperature")
                        → address_space.set_variable_value(
                            "ns=2;s=PLC-1.Boiler1.Temperature",
                            85.2
                          )
                          │
                          │ every 100ms (OPC UA read)
                          ▼
                   SCADA Client
                   session.read(node_id)
                        → PlantState { devices: { "boiler-1": { ... } } }
                        → broadcast::Sender.send(plant_state)
                          │
                          │ on each broadcast
                          ▼
                   WebSocket Handler
                   rx.recv()
                        → JSON: { type, timestamp, devices }
                        → socket.send()
                          │
                          ▼
                   Browser client receives live snapshot
```

---

## Device Topology — Current Water Plant

```
                       Sources (no inputs)
                       ┌──────────────┐    ┌──────────────┐
                       │   boiler-1   │    │   boiler-2   │
                       │  Boiler type │    │  Boiler type │
                       │ target: 85°C │    │ target: 75°C │
                       │ physics:     │    │ physics:     │
                       │ TempRamp 5°/s│    │ TempRamp 5°/s│
                       └──────┬───────┘    └──────┬───────┘
                    pressure ↓│                    │ temperature
                    temp ↓    │                    │ pressure (→ PLC-2)
              ┌───────────────┼────────────────────┘
              │               │
   upstream   │        upstream_pressure
   _pressure  │               │
              ▼               ▼
       ┌──────────────┐  ┌────────────────┐
       │pressure-meter│  │    valve-1     │
       │  Sink type   │  │  Transform     │
       │ physics:     │  │  PressureReg   │
       │ Passthrough  │  │  target: 3 bar │
       │  +1% noise   │  │  kp: 0.02      │
       └──────────────┘  └──────┬─────────┘
                                │
                         position (0.0–1.0)
                         ┌──────┘
                         │ and upstream_pressure (from boiler-1)
                         ▼
                  ┌──────────────┐
                  │ flow-meter-1 │
                  │  Transform   │
                  │ FlowCalc     │
                  │ accumulates  │
                  │  volume      │
                  └──────────────┘

PLC-1 exposes: boiler-1, pressure-meter-1, valve-1   → port 4840
PLC-2 exposes: boiler-2, flow-meter-1                → port 4841
```

---

## Starting the System

**Backend** (terminal 1):
```bash
cd backend
RUST_LOG=info,opcua=warn cargo run --bin water-plant-twin
```

**Frontend** (terminal 2):
```bash
cd frontend
npm run dev          # → http://localhost:5173
```

WebSocket endpoint: `ws://localhost:3000/ws`

---

## Adding a New Device (no Rust changes needed)

1. **Add the schema** to `config/available_devices.json`:
   ```json
   { "device_type": "Tank", "fields": [
     { "name": "level", "data_type": "Double", "description": "Fill level 0–100%" },
     { "name": "status", "data_type": "String", "description": "Normal/Low/Full" }
   ]}
   ```

2. **Add the instance** to `config/devices.json`:
   ```json
   {
     "id": "tank-1", "device_type": "Tank", "category": "sink",
     "input_ports": [{ "name": "inflow", "source_device_id": "valve-1", "source_field": "position" }],
     "output_ports": [],
     "physics_function": { "type": "static" },
     "functions": [],
     "initial_values": { "level": 0.0, "status": "Normal" }
   }
   ```

3. **Update topology** in `config/topology.json` — add `"tank-1"` after `"valve-1"`.

4. **Expose via PLC** in `config/factory.json` — add `tank-1` to a PLC's `device_mappings`.

Restart the backend. Done.

---

## Key Design Decisions

| Decision | Rationale |
|---|---|
| Port-based I/O | Decoupled, declarative device connections; no hardcoded topology |
| JSON-configurable physics | New physics without Rust recompilation |
| Separate `available_devices.json` + `devices.json` | Schema validation distinct from instance configuration |
| OPC UA round-trip (Plant → PLC → SCADA) | Industrial-realistic architecture; SCADA is the canonical state source for the WS bridge |
| `Arc<RwLock<Device>>` in DeviceHandle | Bridges tokio async tasks and opcua std::threads safely |
| Broadcast channel for PlantState | Many WS clients can subscribe independently without coupling |
| Topological execution order | Ensures upstream devices tick before downstream dependents |
