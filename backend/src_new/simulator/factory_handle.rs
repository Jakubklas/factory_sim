use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::models::{DataType, PlantConfig, PlcConfig, DeviceConfig};

// Type aliases to keep signatures readable
type DeviceId = String;
type FieldName = String;
type LiveState = HashMap<DeviceId, HashMap<FieldName, DataType>>;

/// Runtime handle for a loaded plant.
/// Owns both the static config tree and all live device state.
/// Wrap in Arc<RwLock<FactoryHandle>> and share across threads.
pub struct FactoryHandle {
    config: PlantConfig,
    state: LiveState,
}

impl FactoryHandle {
    // -------------------------------------------------------------------------
    // Loading
    // -------------------------------------------------------------------------

    /// Load from JSON and seed live state from each metric's initial_value.
    pub fn from_json(path: &str) -> Result<Arc<RwLock<Self>>, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: PlantConfig = serde_json::from_str(&content)?;

        // Seed live state from initial_value on each metric
        let mut state: LiveState = HashMap::new();
        for plc in &config.plcs {
            for device in &plc.devices {
                let mut fields = HashMap::new();
                for metric in &device.metrics {
                    let value = metric.initial_value.clone()
                        .unwrap_or_else(|| match &metric.data_type {
                            DataType::Float(_)   => DataType::Float(0.0),
                            DataType::Str(_)     => DataType::Str(String::new()),
                            DataType::Boolean(_) => DataType::Boolean(false),
                        });
                    fields.insert(metric.name.clone(), value);
                }
                state.insert(device.device_id.clone(), fields);
            }
        }

        Ok(Arc::new(RwLock::new(Self { config, state })))
    }

    // -------------------------------------------------------------------------
    // Live state — reads
    // -------------------------------------------------------------------------

    /// Get a single field value for a device.
    pub fn get_field(&self, device_id: &str, field: &str) -> Option<&DataType> {
        self.state.get(device_id)?.get(field)
    }

    /// Get all field values for a device.
    pub fn get_device_state(&self, device_id: &str) -> Option<&HashMap<FieldName, DataType>> {
        self.state.get(device_id)
    }

    // -------------------------------------------------------------------------
    // Live state — writes
    // -------------------------------------------------------------------------

    /// Set a single field value on a device.
    pub fn set_field(&mut self, device_id: &str, field: &str, value: DataType) {
        if let Some(fields) = self.state.get_mut(device_id) {
            fields.insert(field.to_string(), value);
        }
    }

    /// Overwrite all fields for a device at once (e.g. after a physics tick).
    pub fn set_device_state(&mut self, device_id: &str, fields: HashMap<FieldName, DataType>) {
        self.state.insert(device_id.to_string(), fields);
    }

    // -------------------------------------------------------------------------
    // Config lookups — PLCs
    // -------------------------------------------------------------------------

    pub fn get_plc_by_id(&self, plc_id: &str) -> Option<&PlcConfig> {
        self.config.plcs.iter().find(|p| p.plc_id == plc_id)
    }

    pub fn get_plc_by_name(&self, name: &str) -> Option<&PlcConfig> {
        self.config.plcs.iter().find(|p| p.name == name)
    }

    // -------------------------------------------------------------------------
    // Config lookups — devices
    // -------------------------------------------------------------------------

    pub fn get_device_by_id(&self, device_id: &str) -> Option<&DeviceConfig> {
        self.config.plcs.iter()
            .flat_map(|plc| plc.devices.iter())
            .find(|d| d.device_id == device_id)
    }

    pub fn get_device_by_name(&self, name: &str) -> Option<&DeviceConfig> {
        self.config.plcs.iter()
            .flat_map(|plc| plc.devices.iter())
            .find(|d| d.name == name)
    }

    pub fn get_device_in_plc(&self, plc_id: &str, device_name: &str) -> Option<&DeviceConfig> {
        self.get_plc_by_id(plc_id)?
            .devices.iter()
            .find(|d| d.name == device_name)
    }

    // -------------------------------------------------------------------------
    // Convenience
    // -------------------------------------------------------------------------

    pub fn plant_name(&self) -> &str {
        &self.config.name
    }

    pub fn default_tick_ms(&self) -> u64 {
        self.config.default_tick_ms
    }

    pub fn all_plcs(&self) -> &[PlcConfig] {
        &self.config.plcs
    }

    pub fn all_devices(&self) -> impl Iterator<Item = &DeviceConfig> {
        self.config.plcs.iter().flat_map(|plc| plc.devices.iter())
    }
}
