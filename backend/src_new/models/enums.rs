use serde::{Deserialize, Serialize, Deserializer};

// ============================================================================
// DataType — field type AND value in one enum
// ============================================================================

/// A field's data type AND its value in one enum.
/// Used both as a schema declaration (what type a metric is)
/// and as a value container (the actual current/initial value).
///
/// Custom Deserialize handles two JSON forms:
///   "Float"  / "Str" / "Boolean"  → type-only marker (value zeroed)
///   20.0     / "Off" / true        → typed value
#[derive(Clone, Debug)]
pub enum DataType {
    Float(f64),
    Str(String),    // named Str to avoid clashing with std::String
    Boolean(bool),
}

impl<'de> Deserialize<'de> for DataType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        match v {
            serde_json::Value::Number(n) => Ok(DataType::Float(n.as_f64().unwrap_or(0.0))),
            serde_json::Value::Bool(b)   => Ok(DataType::Boolean(b)),
            serde_json::Value::String(s) => match s.as_str() {
                "Float"   => Ok(DataType::Float(0.0)),
                "Str"     => Ok(DataType::Str(String::new())),
                "Boolean" => Ok(DataType::Boolean(false)),
                _         => Ok(DataType::Str(s)),
            },
            other => Err(serde::de::Error::custom(
                format!("expected number, bool, or string for DataType, got: {:?}", other)
            )),
        }
    }
}

impl Serialize for DataType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            DataType::Float(f)   => s.serialize_f64(*f),
            DataType::Str(str)   => s.serialize_str(str),
            DataType::Boolean(b) => s.serialize_bool(*b),
        }
    }
}

// ============================================================================
// PhysicsMode
// ============================================================================

/// Whether this device type runs a physics simulation or reads from a real PLC.
/// When Live, the physics_definition script is ignored.
#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub enum PhysicsMode {
    Simulation,  // run physics_definition script each tick
    Live,        // skip physics; tick loop reads raw OPC-UA values instead
}

// ============================================================================
// FunctionKind — externally tagged for clean serde
// ============================================================================

/// Describes a control function a device exposes (e.g. "open", "set_position").
/// Config-only — the actual execution logic lives in simulator/.
///
/// JSON format (externally tagged):
///   "kind": { "SetField":       { "field": "position", "value": 1.0 } }
///   "kind": { "SetFieldFromArg": { "field": "target_temperature", "arg_index": 0 } }
///   "kind": { "IncrementField": { "field": "counter", "amount": 1.0 } }
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum FunctionKind {
    SetField        { field: String, value: DataType },
    SetFieldFromArg { field: String, arg_index: usize },
    IncrementField  { field: String, amount: f64 },
}
