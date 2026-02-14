use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BoilerStatus {
    Off,
    Heating,
    Steady,
    Overheat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Boiler {
    pub id: String,
    pub temperature: f64,
    pub target_temperature: f64,
    pub pressure: f64,
    pub status: BoilerStatus,
}

impl Boiler {
    pub fn new(id: String, target_temperature: f64) -> Self {
        Self {
            id,
            temperature: 20.0,
            target_temperature,
            pressure: 0.0,
            status: BoilerStatus::Off,
        }
    }

    pub fn tick(&mut self, dt: f64) {
        use super::physics::{add_noise, temperature_to_pressure};

        // Ramp temperature toward target
        let temp_diff = self.target_temperature - self.temperature;
        let ramp_rate = 5.0; // degrees per second

        if temp_diff.abs() > 0.1 {
            let change = temp_diff.signum() * ramp_rate * dt;
            self.temperature += change;
            self.temperature = self.temperature.clamp(0.0, 150.0);
            self.status = BoilerStatus::Heating;
        } else {
            self.temperature = self.target_temperature;
            self.status = BoilerStatus::Steady;
        }

        // Check for overheat
        if self.temperature > 120.0 {
            self.status = BoilerStatus::Overheat;
        } else if self.temperature < 10.0 {
            self.status = BoilerStatus::Off;
        }

        // Calculate pressure from temperature
        self.pressure = add_noise(temperature_to_pressure(self.temperature), 2.0);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum MeterStatus {
    Normal,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressureMeter {
    pub id: String,
    pub pressure: f64,
    pub status: MeterStatus,
}

impl PressureMeter {
    pub fn new(id: String) -> Self {
        Self {
            id,
            pressure: 0.0,
            status: MeterStatus::Normal,
        }
    }

    pub fn tick(&mut self, upstream_pressure: f64) {
        use super::physics::add_noise;

        self.pressure = add_noise(upstream_pressure, 1.0);

        // Update status based on pressure
        if self.pressure > 4.5 {
            self.status = MeterStatus::Critical;
        } else if self.pressure > 3.5 {
            self.status = MeterStatus::Warning;
        } else {
            self.status = MeterStatus::Normal;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FlowMeterStatus {
    Normal,
    Low,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowMeter {
    pub id: String,
    pub flow_rate: f64,
    pub total_volume: f64,
    pub status: FlowMeterStatus,
}

impl FlowMeter {
    pub fn new(id: String) -> Self {
        Self {
            id,
            flow_rate: 0.0,
            total_volume: 0.0,
            status: FlowMeterStatus::Normal,
        }
    }

    pub fn tick(&mut self, dt: f64, upstream_pressure: f64, valve_position: f64) {
        use super::physics::{add_noise, calculate_flow_rate};

        self.flow_rate = add_noise(calculate_flow_rate(upstream_pressure, valve_position), 2.0);
        self.total_volume += self.flow_rate * dt / 60.0; // Convert L/min to L

        // Update status based on flow rate
        if self.flow_rate > 40.0 {
            self.status = FlowMeterStatus::High;
        } else if self.flow_rate < 5.0 {
            self.status = FlowMeterStatus::Low;
        } else {
            self.status = FlowMeterStatus::Normal;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValveMode {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValveStatus {
    Open,
    Closed,
    Partial,
    Fault,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Valve {
    pub id: String,
    pub position: f64,
    pub mode: ValveMode,
    pub status: ValveStatus,
}

impl Valve {
    pub fn new(id: String) -> Self {
        Self {
            id,
            position: 0.5,
            mode: ValveMode::Auto,
            status: ValveStatus::Partial,
        }
    }

    pub fn tick(&mut self, upstream_pressure: f64) {
        // In auto mode, regulate based on pressure
        if matches!(self.mode, ValveMode::Auto) {
            let target_pressure = 3.0;
            if upstream_pressure > target_pressure + 0.5 {
                // Pressure too high, open valve more
                self.position = (self.position + 0.02).min(1.0);
            } else if upstream_pressure < target_pressure - 0.5 {
                // Pressure too low, close valve
                self.position = (self.position - 0.02).max(0.0);
            }
        }

        // Update status based on position
        self.status = if self.position > 0.8 {
            ValveStatus::Open
        } else if self.position < 0.2 {
            ValveStatus::Closed
        } else {
            ValveStatus::Partial
        };
    }
}
