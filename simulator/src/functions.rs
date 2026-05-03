use std::collections::HashMap;
use plant_config::{DataType, FunctionKind, DeviceFunctionConfig};

#[allow(dead_code)]
pub fn execute_function(
    func:         &DeviceFunctionConfig,
    device_state: &mut HashMap<String, DataType>,
    args:         &[DataType],
) -> Result<(), String> {
    match &func.kind {
        FunctionKind::SetField { field, value } => {
            device_state.insert(field.clone(), value.clone());
        }
        FunctionKind::SetFieldFromArg { field, arg_index } => {
            let value = args.get(*arg_index)
                .ok_or_else(|| format!("Missing argument at index {}", arg_index))?;
            device_state.insert(field.clone(), value.clone());
        }
        FunctionKind::IncrementField { field, amount } => {
            let current = match device_state.get(field) {
                Some(DataType::Float(v)) => *v,
                _ => return Err(format!("Field '{}' is not a Float", field)),
            };
            device_state.insert(field.clone(), DataType::Float(current + amount));
        }
    }
    Ok(())
}
