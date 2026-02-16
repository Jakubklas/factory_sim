Plan: Stream-Based Device Architecture (Flink-Inspired)
Context
The current factory simulation hardcodes device types throughout the codebase, requiring Rust code changes for each new device. The user wants a Flink-inspired streaming architecture that:

Works for water plants now, CNC shops later
Supports Source/Transform/Sink device types
Allows variable inputs/outputs per device
Enables custom physics functions per device
Enables custom control functions (valve.open(), conveyor.set_speed())
Current bottlenecks:

DeviceHandle enum hardcodes variants (device_handle.rs:7-12)
Device structs hardcoded (devices.rs)
Plant has individual device fields (plant.rs:28-37)
main.rs uses giant match statement (main.rs:129-139)
Rigid TickContext assumes water plant (upstream_pressure, valve_position)
Solution: Stream Processing Device Framework
Key insight: A factory is a dataflow system. Devices are operators (Source → Transform → Sink) that consume inputs, apply physics, and produce outputs.

Architecture Overview

┌──────────────────────────────────────────────────────────────┐
│ Device Categories (like Flink operators)                     │
├──────────────────────────────────────────────────────────────┤
│ Source:     0 inputs  → N outputs  (sensors, heaters)        │
│ Transform:  N inputs  → M outputs  (valves, mixers)          │
│ Sink:       N inputs  → 0 outputs  (displays, loggers)       │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ Flexible I/O (not rigid TickContext)                         │
├──────────────────────────────────────────────────────────────┤
│ InputPort:  {name, source_device_id, source_field}           │
│ OutputPort: {name, target_field}                             │
│ IOContext:  HashMap<String, DeviceFieldValue>                │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ Pluggable Physics                                            │
├──────────────────────────────────────────────────────────────┤
│ trait PhysicsFunction {                                      │
│   fn compute(device, inputs, dt) -> outputs                  │
│ }                                                             │
│ Types: TemperatureRamp, PressurePassthrough,                 │
│        FlowCalculation, PIDController, etc.                  │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ Custom Device Functions                                      │
├──────────────────────────────────────────────────────────────┤
│ valve.call_function("open", [])                              │
│ valve.call_function("set_position", [0.5])                   │
│ conveyor.call_function("set_speed", [100.0])                 │
│ conveyor.call_function("emergency_stop", [])                 │
└──────────────────────────────────────────────────────────────┘
Requirements Fulfillment
✅ Source/Transform/Sink Device Types
Water Plant:

Source: Boiler (generates temperature/pressure)
Transform: Valve (pressure → position/status)
Sink: PressureMeter (reads pressure)
CNC Shop:

Source: Spindle (generates RPM)
Transform: Tool Changer (tool_request → current_tool)
Sink: Display (reads X/Y/Z)
✅ Variable Inputs/Outputs
Boiler: 0 inputs, 2 outputs (temperature, pressure)
Valve: 1 input (upstream_pressure), 2 outputs (position, status)
Mixer: 2 inputs (inlet_a, inlet_b), 1 output (mixed_flow)
3-Way Valve: 2 inputs, 3 outputs
✅ Flexible Physics Configuration
Physics functions in Rust, configured via JSON:


{
  "physics_function": {
    "type": "temperature_ramp",
    "ramp_rate": 5.0,
    "min": 0.0,
    "max": 150.0
  }
}
Multiple devices share same physics with different params.

✅ Custom Functions Per Device
Each device type defines its control functions:


{
  "functions": [
    {"name": "open", "type": "set_field", "field": "position", "value": 1.0},
    {"name": "close", "type": "set_field", "field": "position", "value": 0.0},
    {"name": "set_position", "type": "set_field_from_arg", "field": "position"}
  ]
}
Core Data Structures
1. Device Category

// backend/src/simulator/devices.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceCategory {
    Source,     // No inputs, generates data
    Transform,  // N inputs → M outputs
    Sink,       // Only inputs, no outputs
}
2. Flexible I/O Ports

// backend/src/simulator/devices.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputPort {
    pub name: String,              // e.g., "upstream_pressure"
    pub source_device_id: String,  // Which device
    pub source_field: String,      // Which field
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPort {
    pub name: String,         // e.g., "output_pressure"
    pub target_field: String, // Internal field name
}
Example - Mixer with 2 inputs, 1 output:


{
  "input_ports": [
    {"name": "inlet_a", "source_device_id": "tank-1", "source_field": "flow"},
    {"name": "inlet_b", "source_device_id": "tank-2", "source_field": "flow"}
  ],
  "output_ports": [
    {"name": "mixed_flow", "target_field": "output_flow"}
  ]
}
3. Physics Function System

// backend/src/simulator/physics_functions.rs (new file)

pub trait PhysicsFunction: Send + Sync {
    fn compute(
        &self,
        device: &Device,
        inputs: &HashMap<String, DeviceFieldValue>,
        dt: f64,
    ) -> HashMap<String, DeviceFieldValue>;  // Port outputs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhysicsFunctionConfig {
    TemperatureRamp {
        ramp_rate: f64,
        min: f64,
        max: f64,
        pressure_from_temp: bool,
    },
    PressurePassthrough {
        noise_percent: f64,
    },
    FlowCalculation {
        coefficient: f64,
        accumulate_volume: bool,
    },
    PressureRegulator {
        target_pressure: f64,
        kp: f64,  // Proportional gain
    },
    PIDController {
        kp: f64,
        ki: f64,
        kd: f64,
        setpoint: f64,
    },
    Static,  // No automatic updates
}

pub fn create_physics_function(config: PhysicsFunctionConfig) -> Box<dyn PhysicsFunction> {
    match config {
        PhysicsFunctionConfig::TemperatureRamp { .. } => {
            Box::new(TemperatureRampPhysics::from_config(config))
        }
        PhysicsFunctionConfig::PressurePassthrough { .. } => {
            Box::new(PassthroughPhysics::from_config(config))
        }
        // ... etc
    }
}
Implementation Example - Temperature Ramp:


struct TemperatureRampPhysics {
    ramp_rate: f64,
    min: f64,
    max: f64,
    pressure_from_temp: bool,
}

impl PhysicsFunction for TemperatureRampPhysics {
    fn compute(
        &self,
        device: &Device,
        _inputs: &HashMap<String, DeviceFieldValue>,  // Source device, no inputs
        dt: f64,
    ) -> HashMap<String, DeviceFieldValue> {
        let mut outputs = HashMap::new();

        // Read current state (reuse Boiler::tick logic from line 38-64)
        let temp = device.get_float("temperature").unwrap_or(20.0);
        let target = device.get_float("target_temperature").unwrap_or(20.0);

        // Ramp temperature
        let temp_diff = target - temp;
        let new_temp = if temp_diff.abs() > 0.1 {
            let change = temp_diff.signum() * self.ramp_rate * dt;
            (temp + change).clamp(self.min, self.max)
        } else {
            target
        };

        outputs.insert("temperature".to_string(), DeviceFieldValue::Float(new_temp));

        // Calculate pressure from temperature if enabled
        if self.pressure_from_temp {
            let pressure = physics::temperature_to_pressure(new_temp);
            outputs.insert("pressure".to_string(), DeviceFieldValue::Float(pressure));
        }

        // Status
        let status = if new_temp > 120.0 {
            "Overheat"
        } else if new_temp < 10.0 {
            "Off"
        } else if temp_diff.abs() > 0.1 {
            "Heating"
        } else {
            "Steady"
        };
        outputs.insert("status".to_string(), DeviceFieldValue::String(status.to_string()));

        outputs
    }
}
4. Device Function System

// backend/src/simulator/device_functions.rs (new file)

pub trait DeviceFunction: Send + Sync {
    fn execute(&self, device: &mut Device, args: Vec<DeviceFieldValue>) -> Result<(), String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeviceFunctionConfig {
    SetField {
        field: String,
        value: serde_json::Value,  // JSON value (float/string)
    },
    SetFieldFromArg {
        field: String,
        arg_index: usize,
    },
    IncrementField {
        field: String,
        amount: f64,
    },
}

// Simple function: set field to fixed value
struct SetFieldFunction {
    field: String,
    value: DeviceFieldValue,
}

impl DeviceFunction for SetFieldFunction {
    fn execute(&self, device: &mut Device, _args: Vec<DeviceFieldValue>) -> Result<(), String> {
        device.set_field(self.field.clone(), self.value.clone());
        Ok(())
    }
}

// Function: set field from argument
struct SetFieldFromArgFunction {
    field: String,
    arg_index: usize,
}

impl DeviceFunction for SetFieldFromArgFunction {
    fn execute(&self, device: &mut Device, args: Vec<DeviceFieldValue>) -> Result<(), String> {
        let value = args.get(self.arg_index)
            .ok_or_else(|| format!("Missing arg at index {}", self.arg_index))?;
        device.set_field(self.field.clone(), value.clone());
        Ok(())
    }
}
5. Generic Device

// backend/src/simulator/devices.rs

pub struct Device {
    pub id: String,
    pub device_type: String,
    pub category: DeviceCategory,

    // Dynamic field storage
    fields: HashMap<String, DeviceFieldValue>,

    // I/O configuration
    input_ports: Vec<InputPort>,
    output_ports: Vec<OutputPort>,

    // Physics/logic
    physics_function: Box<dyn PhysicsFunction>,

    // Control functions
    functions: HashMap<String, Box<dyn DeviceFunction>>,

    // Schema reference
    #[serde(skip)]
    schema: Option<DeviceSchema>,
}

impl Device {
    pub fn new(
        id: String,
        device_type: String,
        category: DeviceCategory,
        schema: DeviceSchema,
        input_ports: Vec<InputPort>,
        output_ports: Vec<OutputPort>,
        physics_config: PhysicsFunctionConfig,
        function_configs: Vec<(String, DeviceFunctionConfig)>,
        initial_values: HashMap<String, DeviceFieldValue>,
    ) -> Result<Self, String> {
        // Initialize fields from schema + initial values
        let mut fields = HashMap::new();
        for field_schema in &schema.fields {
            let value = initial_values
                .get(&field_schema.name)
                .cloned()
                .unwrap_or_else(|| match field_schema.data_type {
                    DataType::Double => DeviceFieldValue::Float(0.0),
                    DataType::String => DeviceFieldValue::String("Unknown".to_string()),
                });
            fields.insert(field_schema.name.clone(), value);
        }

        // Create physics function
        let physics_function = create_physics_function(physics_config);

        // Create control functions
        let functions = function_configs
            .into_iter()
            .map(|(name, config)| (name, create_device_function(config)))
            .collect();

        Ok(Self {
            id,
            device_type,
            category,
            fields,
            input_ports,
            output_ports,
            physics_function,
            functions,
            schema: Some(schema),
        })
    }

    pub fn tick(&mut self, inputs: &HashMap<String, DeviceFieldValue>, dt: f64) {
        // 1. Compute outputs using physics function
        let outputs = self.physics_function.compute(self, inputs, dt);

        // 2. Write outputs to internal fields via output ports
        for (port_name, value) in outputs {
            if let Some(port) = self.output_ports.iter().find(|p| p.name == port_name) {
                self.fields.insert(port.target_field.clone(), value);
            }
        }
    }

    pub fn call_function(&mut self, name: &str, args: Vec<DeviceFieldValue>) -> Result<(), String> {
        let func = self.functions.get(name)
            .ok_or_else(|| format!("Function '{}' not found on device '{}'", name, self.id))?;
        func.execute(self, args)
    }

    // Field access helpers
    pub fn get_field(&self, name: &str) -> Option<DeviceFieldValue> {
        self.fields.get(name).cloned()
    }

    pub fn set_field(&mut self, name: String, value: DeviceFieldValue) {
        self.fields.insert(name, value);
    }

    pub fn get_float(&self, name: &str) -> Option<f64> {
        match self.fields.get(name)? {
            DeviceFieldValue::Float(v) => Some(*v),
            _ => None,
        }
    }
}

impl DeviceFields for Device {
    fn get_field(&self, field_name: &str) -> Option<DeviceFieldValue> {
        self.fields.get(field_name).cloned()
    }
}
6. Plant (Dataflow Graph)

// backend/src/simulator/plant.rs

pub struct Plant {
    devices: HashMap<String, Device>,
    execution_order: Vec<String>,  // Topologically sorted
}

impl Plant {
    pub fn tick(&mut self, dt: f64) {
        for device_id in &self.execution_order.clone() {
            // 1. Gather inputs from upstream devices via input ports
            let device = self.devices.get(device_id).unwrap();
            let mut inputs = HashMap::new();

            for input_port in &device.input_ports {
                if let Some(source_dev) = self.devices.get(&input_port.source_device_id) {
                    if let Some(value) = source_dev.get_field(&input_port.source_field) {
                        inputs.insert(input_port.name.clone(), value);
                    }
                }
            }

            // 2. Tick the device with gathered inputs
            let device = self.devices.get_mut(device_id).unwrap();
            device.tick(&inputs, dt);
        }
    }

    pub fn from_config(
        device_configs: Vec<DeviceConfig>,
        topology: Topology,
        schema_registry: &DeviceSchemaRegistry,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Create devices from configs
        let mut devices = HashMap::new();
        for config in device_configs {
            let schema = schema_registry.get_schema(&config.device_type)?.clone();
            let device = Device::new(
                config.id.clone(),
                config.device_type,
                config.category,
                schema,
                config.input_ports,
                config.output_ports,
                config.physics_function,
                config.functions,
                config.initial_values,
            )?;
            devices.insert(config.id, device);
        }

        // Compute execution order (topological sort of input dependencies)
        let execution_order = topology.compute_execution_order(&devices)?;

        Ok(Self {
            devices,
            execution_order,
        })
    }

    pub fn get_device(&self, id: &str) -> Option<&Device> {
        self.devices.get(id)
    }

    pub fn get_device_mut(&mut self, id: &str) -> Option<&mut Device> {
        self.devices.get_mut(id)
    }

    pub fn get_state(&self) -> PlantState {
        PlantState {
            devices: self.devices.clone(),
        }
    }

    pub fn call_device_function(
        &mut self,
        device_id: &str,
        function_name: &str,
        args: Vec<DeviceFieldValue>,
    ) -> Result<(), String> {
        let device = self.devices.get_mut(device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        device.call_function(function_name, args)
    }
}

#[derive(Debug, Clone)]
pub struct PlantState {
    pub devices: HashMap<String, Device>,
}
JSON Configuration
devices.json (new)

{
  "devices": [
    {
      "id": "boiler-1",
      "device_type": "Boiler",
      "category": "source",
      "input_ports": [],
      "output_ports": [
        {"name": "temperature", "target_field": "temperature"},
        {"name": "pressure", "target_field": "pressure"}
      ],
      "physics_function": {
        "type": "temperature_ramp",
        "ramp_rate": 5.0,
        "min": 0.0,
        "max": 150.0,
        "pressure_from_temp": true
      },
      "functions": [],
      "initial_values": {
        "temperature": 20.0,
        "target_temperature": 85.0,
        "pressure": 0.0,
        "status": "Off"
      }
    },
    {
      "id": "valve-1",
      "device_type": "Valve",
      "category": "transform",
      "input_ports": [
        {
          "name": "upstream_pressure",
          "source_device_id": "boiler-1",
          "source_field": "pressure"
        }
      ],
      "output_ports": [
        {"name": "position", "target_field": "position"},
        {"name": "status", "target_field": "status"}
      ],
      "physics_function": {
        "type": "pressure_regulator",
        "target_pressure": 3.0,
        "kp": 0.02
      },
      "functions": [
        {"name": "open", "type": "set_field", "field": "position", "value": 1.0},
        {"name": "close", "type": "set_field", "field": "position", "value": 0.0},
        {"name": "set_position", "type": "set_field_from_arg", "field": "position", "arg_index": 0}
      ],
      "initial_values": {
        "position": 0.5,
        "mode": "Auto",
        "status": "Partial"
      }
    },
    {
      "id": "pressure-meter-1",
      "device_type": "PressureMeter",
      "category": "sink",
      "input_ports": [
        {
          "name": "upstream_pressure",
          "source_device_id": "boiler-1",
          "source_field": "pressure"
        }
      ],
      "output_ports": [],
      "physics_function": {
        "type": "pressure_passthrough",
        "noise_percent": 1.0
      },
      "functions": [],
      "initial_values": {
        "pressure": 0.0,
        "status": "Normal"
      }
    }
  ]
}
topology.json (simplified)
With input ports defined in devices.json, topology just needs execution order:


{
  "execution_order": [
    "boiler-1",
    "boiler-2",
    "pressure-meter-1",
    "valve-1",
    "flow-meter-1"
  ]
}
OR auto-compute from input_ports (topological sort).

Implementation Plan
Phase 1: Create Physics Function System
New file: backend/src/simulator/physics_functions.rs

Define PhysicsFunction trait
Implement physics functions (port existing logic):
TemperatureRampPhysics (from Boiler::tick)
PassthroughPhysics (from PressureMeter::tick)
FlowCalculationPhysics (from FlowMeter::tick)
PressureRegulatorPhysics (from Valve::tick)
Create factory function
Phase 2: Create Device Function System
New file: backend/src/simulator/device_functions.rs

Define DeviceFunction trait
Implement function types:
SetFieldFunction
SetFieldFromArgFunction
IncrementFieldFunction
Create factory function
Phase 3: Update Device Structure
File: backend/src/simulator/devices.rs

Add DeviceCategory enum
Add InputPort / OutputPort structs
Update Device struct (keep old devices temporarily for reference)
Implement Device::new(), Device::tick(), Device::call_function()
Implement DeviceFields trait
Phase 4: Update Plant Structure
File: backend/src/simulator/plant.rs

Replace Plant with HashMap<String, Device>
Add execution_order field
Implement Plant::from_config()
Update Plant::tick() to use port-based I/O
Add Plant::call_device_function()
Phase 5: Replace DeviceHandle
File: backend/src/models/devices/device_handle.rs

Replace enum with generic wrapper (no changes to OPC UA server needed):


pub struct DeviceHandle {
    device: Arc<RwLock<Device>>,
}

impl DeviceHandle {
    pub fn new(device: Arc<RwLock<Device>>) -> Self { ... }
    pub async fn read_field(&self, field_name: &str) -> Option<DeviceFieldValue> { ... }
    pub fn get_device(&self) -> Arc<RwLock<Device>> { ... }
    pub async fn call_function(&self, name: &str, args: Vec<DeviceFieldValue>) -> Result<(), String> {
        let mut device = self.device.write().await;
        device.call_function(name, args)
    }
}
Phase 6: Update main.rs
File: backend/src/main.rs

Load configs:


let schema_registry = DeviceSchemaRegistry::from_json("backend/config/available_devices.json")?;
let device_configs = DeviceConfigRegistry::from_json("backend/config/devices.json")?;
let topology = Topology::from_json("backend/config/topology.json")?;
Create plant:


let plant = Arc::new(RwLock::new(
    Plant::from_config(device_configs.devices, topology, &schema_registry)?
));
Create device handles (REMOVE match statement at lines 129-139):


let mut device_handles = HashMap::new();
let plant_state = plant.read().await.get_state();

for (device_id, device) in plant_state.devices {
    let device_arc = Arc::new(RwLock::new(device));
    device_handles.insert(device_id, DeviceHandle::new(device_arc));
}
Start PLC servers (no match statement needed):


for device_mapping in &plc_config.device_mappings {
    if let Some(handle) = device_handles.get(&device_mapping.device_id) {
        devices.insert(device_mapping.device_id.clone(), handle.clone());
    }
}
Update sync task (replace lines 84-96):


tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_millis(50));
    loop {
        interval.tick().await;
        let plant = plant_sync.read().await;

        for (device_id, handle) in &device_handles_sync {
            if let Some(device) = plant.get_device(device_id) {
                *handle.get_device().write().await = device.clone();
            }
        }
    }
});
Phase 7: Create JSON Configs
devices.json - Define all device instances with ports, physics, functions
topology.json - Define execution order (or auto-compute)
Keep factory.json as-is (PLC mappings)
Phase 8: Add Supporting Types
New file: backend/src/models/devices/device_config.rs


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub id: String,
    pub device_type: String,
    pub category: DeviceCategory,
    pub input_ports: Vec<InputPort>,
    pub output_ports: Vec<OutputPort>,
    pub physics_function: PhysicsFunctionConfig,
    pub functions: Vec<(String, DeviceFunctionConfig)>,
    pub initial_values: HashMap<String, DeviceFieldValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfigRegistry {
    pub devices: Vec<DeviceConfig>,
}

impl DeviceConfigRegistry {
    pub fn from_json(path: &str) -> Result<Self, Box<dyn std::error::Error>> { ... }
}
Phase 9: Cleanup
Once tested:

Delete old device structs (Boiler, Valve, PressureMeter, FlowMeter)
Delete old Plant::new() with individual fields
Update imports
Remove PKI directories: rm -rf backend/pki*, add backend/pki*/ to .gitignore