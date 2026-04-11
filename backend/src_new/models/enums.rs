/// A field's data type AND its value in one enum.
/// Used both as a schema declaration (what type a metric is)
/// and as a value container (the actual current/initial value).
pub enum DataType {
    Float(f64),
    Str(String),    // named Str to avoid clashing with std::String
    Boolean(bool),
}

/// Whether this device type runs a physics simulation or reads from a real PLC.
/// When Live, the physics_definition script is ignored — OPC-UA writes
/// state directly into FactoryHandle every tick.
pub enum PhysicsMode {
    Simulation,  // run physics_definition script each tick
    Live,        // skip physics; tick loop reads raw OPC-UA values instead
}

/// Describes a control function a device exposes (e.g. "open", "set_position").
/// Config-only — the actual execution logic lives in simulator/.
pub enum FunctionKind {
    SetField { field: String, value: DataType },
    SetFieldFromArg { field: String, arg_index: usize },
    IncrementField { field: String, amount: f64 },
}
