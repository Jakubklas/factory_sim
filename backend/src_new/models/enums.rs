/// The type of physical device. Add variants here as new device types are supported.
/// Using an enum (vs a String) gives compile-time exhaustiveness checks.
pub enum DeviceType {
    Pump,
    Boiler,
    Valve,
    // Custom(String),  // uncomment if device types ever become user-defined
}

/// How a device participates in the simulation graph.
/// Mirrors stream-processing semantics (Flink-style):
///   Source    — produces data, no inputs     (e.g. boiler, sensor)
///   Transform — consumes + produces data     (e.g. valve, mixer)
///   Sink      — consumes data, no outputs    (e.g. logger, display)
pub enum DeviceCategory {
    Source,
    Transform,
    Sink,
}

/// A field's data type AND its value in one enum.
/// Used both as a schema declaration (what type a metric is)
/// and as a value container (the actual current/initial value).
/// NOTE: this double duty is intentional for brevity — revisit if
/// schema validation needs to be separated from value storage.
pub enum DataType {
    Float(f64),
    Str(String),    // named Str to avoid clashing with std::String
    Boolean(bool),
}

/// Describes the physics model to use for a device.
/// Config-only — the actual computation lives in simulator/.
pub enum PhysicsKind {
    TemperatureRamp { ramp_rate: f64, min: f64, max: f64, pressure_from_temp: bool },
    PressurePassthrough { noise_percent: f64 },
    FlowCalculation { coefficient: f64, accumulate_volume: bool },
    PressureRegulator { target_pressure: f64, kp: f64 },
    Static,
}

/// Describes a control function a device exposes (e.g. "open", "set_position").
/// Config-only — the actual execution logic lives in simulator/.
pub enum FunctionKind {
    SetField { field: String, value: DataType },
    SetFieldFromArg { field: String, arg_index: usize },
    IncrementField { field: String, amount: f64 },
}
