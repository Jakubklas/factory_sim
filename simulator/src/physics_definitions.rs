use std::collections::HashMap;
use rhai::{Dynamic, Engine, Scope, AST};
use plant_config::{DataType, DeviceTypeDefinition};

/// Compiles all device physics scripts once at startup, then executes them per tick.
///
/// Scripts are Rhai code stored as strings in device_types.json.
/// Each script receives: `state` (Map), `params` (Map), `dt` (f64).
pub struct PhysicsEngine {
    engine:  Engine,
    scripts: HashMap<String, AST>,
}

impl PhysicsEngine {
    pub fn new(device_types: &[DeviceTypeDefinition]) -> Result<Self, Box<dyn std::error::Error>> {
        let engine = Engine::new();
        let mut scripts = HashMap::new();

        for type_def in device_types {
            if let Some(script) = &type_def.physics_definition {
                let ast = engine.compile(script).map_err(|e| {
                    format!("Physics script compile error for '{}': {}", type_def.device_type, e)
                })?;
                scripts.insert(type_def.device_type.clone(), ast);
            }
        }

        Ok(Self { engine, scripts })
    }

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
