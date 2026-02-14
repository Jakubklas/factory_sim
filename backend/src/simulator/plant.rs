use super::devices::{Boiler, PressureMeter, FlowMeter, Valve};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlantState {
    #[serde(flatten)]
    pub devices: HashMap<String, DeviceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeviceState {
    Boiler(Boiler),
    PressureMeter(PressureMeter),
    FlowMeter(FlowMeter),
    Valve(Valve),
}

pub struct Plant {
    pub boiler_1: Boiler,
    pub boiler_2: Boiler,
    pub pressure_meter_1: PressureMeter,
    pub flow_meter_1: FlowMeter,
    pub valve_1: Valve,
}

impl Plant {
    pub fn new() -> Self {
        Self {
            boiler_1: Boiler::new("boiler-1".to_string(), 85.0),
            boiler_2: Boiler::new("boiler-2".to_string(), 75.0),
            pressure_meter_1: PressureMeter::new("pressure-meter-1".to_string()),
            flow_meter_1: FlowMeter::new("flow-meter-1".to_string()),
            valve_1: Valve::new("valve-1".to_string()),
        }
    }

    pub fn tick(&mut self, dt: f64) {
        // Update devices in topology order following the plant flow
        // Boiler 1 → Pressure Meter 1 → Valve 1 → Flow Meter 1 → Boiler 2

        self.boiler_1.tick(dt);
        self.pressure_meter_1.tick(self.boiler_1.pressure);
        self.valve_1.tick(self.boiler_1.pressure);
        self.flow_meter_1.tick(dt, self.boiler_1.pressure, self.valve_1.position);
        self.boiler_2.tick(dt);
    }

    pub fn get_state(&self) -> PlantState {
        let mut devices = HashMap::new();
        devices.insert("boiler-1".to_string(), DeviceState::Boiler(self.boiler_1.clone()));
        devices.insert("boiler-2".to_string(), DeviceState::Boiler(self.boiler_2.clone()));
        devices.insert("pressure-meter-1".to_string(), DeviceState::PressureMeter(self.pressure_meter_1.clone()));
        devices.insert("flow-meter-1".to_string(), DeviceState::FlowMeter(self.flow_meter_1.clone()));
        devices.insert("valve-1".to_string(), DeviceState::Valve(self.valve_1.clone()));

        PlantState { devices }
    }
}
