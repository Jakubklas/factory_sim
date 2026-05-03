use std::collections::HashMap;
use plant_config::{DataType, ResolvedDevice};

pub type DeviceId  = String;
pub type FieldName = String;

/// Private simulator state: live field values for all simulated devices.
/// Seeded from metric initial_values at startup; mutated every tick.
pub struct SimulatorState {
    pub values: HashMap<DeviceId, HashMap<FieldName, DataType>>,
}

impl SimulatorState {
    pub fn new(devices: &[ResolvedDevice]) -> Self {
        let mut values: HashMap<DeviceId, HashMap<FieldName, DataType>> = HashMap::new();
        for device in devices {
            let mut fields: HashMap<FieldName, DataType> = HashMap::new();
            for metric in &device.type_def.metrics {
                let value = metric.initial_value.clone()
                    .unwrap_or_else(|| match &metric.data_type {
                        DataType::Float(_)   => DataType::Float(0.0),
                        DataType::Str(_)     => DataType::Str(String::new()),
                        DataType::Boolean(_) => DataType::Boolean(false),
                    });
                fields.insert(metric.name.clone(), value);
            }
            values.insert(device.config.device_id.clone(), fields);
        }
        Self { values }
    }

    pub fn get_field(&self, device_id: &str, field: &str) -> Option<&DataType> {
        self.values.get(device_id)?.get(field)
    }

    pub fn get_device_state(&self, device_id: &str) -> Option<&HashMap<FieldName, DataType>> {
        self.values.get(device_id)
    }

    pub fn set_field(&mut self, device_id: &str, field: &str, value: DataType) {
        if let Some(fields) = self.values.get_mut(device_id) {
            fields.insert(field.to_string(), value);
        }
    }

    pub fn set_device_state(&mut self, device_id: &str, fields: HashMap<FieldName, DataType>) {
        self.values.insert(device_id.to_string(), fields);
    }

    /// Snapshot for plc_server's address-space update loop.
    pub fn snapshot(&self) -> HashMap<DeviceId, HashMap<FieldName, DataType>> {
        self.values.clone()
    }
}
