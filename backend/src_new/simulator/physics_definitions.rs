use std::collections::HashMap;
use rhai::{Dynamic, Engine, Scope, AST};
use crate::models::{DataType, DeviceTypeDefinition};

/// Compiles all device physics scripts once at startup, then executes them per tick.
///
/// Scripts are Rhai code stored as strings in device_types.json — fully user-editable.
/// No host functions are registered: all logic is plain Rhai.
/// PhysicsMode::Live devices are skipped — their AST is never compiled.
///
/// Each script receives three variables:
///   `state`  — Map of the device's current field values (read + write)
///   `params` — Map of the instance's numeric params (read-only)
///   `dt`     — Elapsed seconds since last tick (f64, read-only)
///
/// Rhai built-ins available in scripts: min(), max(), abs(), floor(), ceil(),
/// round(), sqrt(), loops (for/while/loop), conditionals, closures, etc.
pub struct PhysicsEngine {
    engine:  Engine,
    scripts: HashMap<String, AST>,   // device_type → compiled AST
}

impl PhysicsEngine {
    /// Compile all Simulated physics scripts.
    /// Fails fast if any script has a syntax error — call once at startup.
    pub fn new(device_types: &[DeviceTypeDefinition]) -> Result<Self, Box<dyn std::error::Error>> {
        let engine = Engine::new();

        // ----------------------------------------------------------------
        // Compile scripts
        // ----------------------------------------------------------------

        let mut scripts = HashMap::new();
        for type_def in device_types {
            if let Some(script) = &type_def.physics_definition {
                let ast = engine.compile(script).map_err(|e| {
                    format!(
                        "Physics script compile error for '{}': {}",
                        type_def.device_type, e
                    )
                })?;
                scripts.insert(type_def.device_type.clone(), ast);
            }
        }

        Ok(Self { engine, scripts })
    }

    /// Execute the physics script for one device.
    ///
    /// `state`  — device's live fields; script reads and writes this.
    /// `params` — instance params from DeviceConfig; read-only inside the script.
    /// `dt`     — seconds elapsed since last tick.
    ///
    /// If no script exists for this device type (LivePLC or no definition),
    /// returns Ok(()) immediately without touching state.
    pub fn run(
        &self,
        device_type: &str,
        state:       &mut HashMap<String, DataType>,
        params:      &HashMap<String, f64>,
        dt:          f64,
    ) -> Result<(), String> {
        let ast = match self.scripts.get(device_type) {
            Some(ast) => ast,
            None      => return Ok(()),
        };

        // Convert state → Rhai Map
        let rhai_state: rhai::Map = state
            .iter()
            .map(|(k, v)| {
                let dyn_val: Dynamic = match v {
                    DataType::Float(f)   => Dynamic::from(*f),
                    DataType::Str(s)     => Dynamic::from(s.clone()),
                    DataType::Boolean(b) => Dynamic::from(*b),
                };
                (k.as_str().into(), dyn_val)
            })
            .collect();

        // Convert params → Rhai Map (all values are f64)
        let rhai_params: rhai::Map = params
            .iter()
            .map(|(k, v)| (k.as_str().into(), Dynamic::from(*v)))
            .collect();

        let mut scope = Scope::new();
        scope.push("state",  rhai_state);
        scope.push("params", rhai_params);
        scope.push("dt",     dt);

        self.engine
            .run_ast_with_scope(&mut scope, ast)
            .map_err(|e| format!("Physics runtime error for '{}': {}", device_type, e))?;

        // Write updated state back — only fields that already exist in state.
        // This prevents scripts from injecting unexpected keys.
        if let Some(updated) = scope.get_value::<rhai::Map>("state") {
            for (k, v) in updated {
                let key = k.as_str();
                if let Some(existing) = state.get(key) {
                    let new_val: Option<DataType> = match existing {
                        DataType::Float(_)   => v.as_float().ok().map(DataType::Float),
                        DataType::Boolean(_) => v.as_bool().ok().map(DataType::Boolean),
                        DataType::Str(_)     => v.try_cast::<String>().map(DataType::Str),
                    };
                    if let Some(val) = new_val {
                        state.insert(key.to_string(), val);
                    }
                }
            }
        }

        Ok(())
    }
}
