use std::collections::HashMap;
use crate::models::{DataType, DeviceFunctionConfig};

/// Execute a control function against a device's current live state.
/// Called on-demand (from SCADA command, UI, API) — not every tick.
///
/// `state` is the live field map for a single device from FactoryHandle.
pub fn execute_function(
    func: &DeviceFunctionConfig,
    device_state: &mut HashMap<String, DataType>,
    args: &[DataType],
) -> Result<(), String> {
    match &func.kind {
        crate::models::FunctionKind::SetField { field, value } => {
            device_state.insert(field.clone(), value.clone());
        }

        crate::models::FunctionKind::SetFieldFromArg { field, arg_index } => {
            let value = args.get(*arg_index)
                .ok_or_else(|| format!("Missing argument at index {}", arg_index))?;
            device_state.insert(field.clone(), value.clone());
        }

        crate::models::FunctionKind::IncrementField { field, amount } => {
            let current = match device_state.get(field) {
                Some(DataType::Float(v)) => *v,
                _ => return Err(format!("Field '{}' is not a Float", field)),
            };
            device_state.insert(field.clone(), DataType::Float(current + amount));
        }
    }

    Ok(())
}
