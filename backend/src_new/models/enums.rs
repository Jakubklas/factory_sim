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
pub enum DataType {
    Float(f64),
    Str(String),    // named Str to avoid clashing with std::String
    Boolean(bool),
}

/// Describes the physics model to use for a device type.
/// Numeric params are NOT stored here — they come from DeviceConfig.params
/// at load time, validated against DeviceTypeDefinition.required_params.
/// Config-only — the actual computation lives in simulator/.
pub enum PhysicsKind {
    TemperatureRamp,
    PressurePassthrough,
    FlowCalculation,
    PressureRegulator,
    Static,
}

/// Describes a control function a device exposes (e.g. "open", "set_position").
/// Config-only — the actual execution logic lives in simulator/.
pub enum FunctionKind {
    SetField { field: String, value: DataType },
    SetFieldFromArg { field: String, arg_index: usize },
    IncrementField { field: String, amount: f64 },
}
