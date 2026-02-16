use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use crate::models::DeviceFieldValue;
use super::physics;

/// Trait for physics functions that compute device outputs from inputs
pub trait PhysicsFunction: Send + Sync {
    fn compute(
        &self,
        device: &super::devices::Device,
        inputs: &HashMap<String, DeviceFieldValue>,
        dt: f64,
    ) -> HashMap<String, DeviceFieldValue>;
}

/// Configuration for physics functions - loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhysicsFunctionConfig {
    TemperatureRamp {
        ramp_rate: f64,
        min: f64,
        max: f64,
        pressure_from_temp: bool,
    },
    PressurePassthrough {
        noise_percent: f64,
    },
    FlowCalculation {
        coefficient: f64,
        accumulate_volume: bool,
    },
    PressureRegulator {
        target_pressure: f64,
        kp: f64,  // Proportional gain
    },
    Static,  // No automatic updates
}

/// Factory function to create physics functions from config
pub fn create_physics_function(config: PhysicsFunctionConfig) -> Arc<dyn PhysicsFunction> {
    match config {
        PhysicsFunctionConfig::TemperatureRamp { ramp_rate, min, max, pressure_from_temp } => {
            Arc::new(TemperatureRampPhysics { ramp_rate, min, max, pressure_from_temp })
        }
        PhysicsFunctionConfig::PressurePassthrough { noise_percent } => {
            Arc::new(PassthroughPhysics { noise_percent })
        }
        PhysicsFunctionConfig::FlowCalculation { coefficient, accumulate_volume } => {
            Arc::new(FlowCalculationPhysics { coefficient, accumulate_volume })
        }
        PhysicsFunctionConfig::PressureRegulator { target_pressure, kp } => {
            Arc::new(PressureRegulatorPhysics { target_pressure, kp })
        }
        PhysicsFunctionConfig::Static => {
            Arc::new(StaticPhysics)
        }
    }
}

// ============================================================================
// Temperature Ramp Physics (ported from Boiler::tick)
// ============================================================================

struct TemperatureRampPhysics {
    ramp_rate: f64,
    min: f64,
    max: f64,
    pressure_from_temp: bool,
}

impl PhysicsFunction for TemperatureRampPhysics {
    fn compute(
        &self,
        device: &super::devices::Device,
        _inputs: &HashMap<String, DeviceFieldValue>,
        dt: f64,
    ) -> HashMap<String, DeviceFieldValue> {
        let mut outputs = HashMap::new();

        // Read current state
        let temp = device.get_float("temperature").unwrap_or(20.0);
        let target = device.get_float("target_temperature").unwrap_or(20.0);

        // Ramp temperature toward target (from devices.rs:38-64)
        let temp_diff = target - temp;
        let new_temp = if temp_diff.abs() > 0.1 {
            let change = temp_diff.signum() * self.ramp_rate * dt;
            (temp + change).clamp(self.min, self.max)
        } else {
            target
        };

        outputs.insert("temperature".to_string(), DeviceFieldValue::Float(new_temp));

        // Calculate pressure from temperature if enabled
        if self.pressure_from_temp {
            let pressure = physics::add_noise(physics::temperature_to_pressure(new_temp), 2.0);
            outputs.insert("pressure".to_string(), DeviceFieldValue::Float(pressure));
        }

        // Update status based on temperature
        let status = if new_temp > 120.0 {
            "Overheat"
        } else if new_temp < 10.0 {
            "Off"
        } else if temp_diff.abs() > 0.1 {
            "Heating"
        } else {
            "Steady"
        };
        outputs.insert("status".to_string(), DeviceFieldValue::String(status.to_string()));

        outputs
    }
}

// ============================================================================
// Pressure Passthrough Physics (ported from PressureMeter::tick)
// ============================================================================

struct PassthroughPhysics {
    noise_percent: f64,
}

impl PhysicsFunction for PassthroughPhysics {
    fn compute(
        &self,
        _device: &super::devices::Device,
        inputs: &HashMap<String, DeviceFieldValue>,
        _dt: f64,
    ) -> HashMap<String, DeviceFieldValue> {
        let mut outputs = HashMap::new();

        // Read upstream pressure (from devices.rs:103-117)
        if let Some(DeviceFieldValue::Float(upstream_pressure)) = inputs.get("upstream_pressure") {
            let pressure = physics::add_noise(*upstream_pressure, self.noise_percent);
            outputs.insert("pressure".to_string(), DeviceFieldValue::Float(pressure));

            // Update status based on pressure thresholds
            let status = if pressure > 4.5 {
                "Critical"
            } else if pressure > 3.5 {
                "Warning"
            } else {
                "Normal"
            };
            outputs.insert("status".to_string(), DeviceFieldValue::String(status.to_string()));
        }

        outputs
    }
}

// ============================================================================
// Flow Calculation Physics (ported from FlowMeter::tick)
// ============================================================================

struct FlowCalculationPhysics {
    coefficient: f64,
    accumulate_volume: bool,
}

impl PhysicsFunction for FlowCalculationPhysics {
    fn compute(
        &self,
        device: &super::devices::Device,
        inputs: &HashMap<String, DeviceFieldValue>,
        dt: f64,
    ) -> HashMap<String, DeviceFieldValue> {
        let mut outputs = HashMap::new();

        // Read inputs (from devices.rs:155-169)
        let upstream_pressure = inputs.get("upstream_pressure")
            .and_then(|v| if let DeviceFieldValue::Float(f) = v { Some(*f) } else { None })
            .unwrap_or(0.0);

        let valve_position = inputs.get("valve_position")
            .and_then(|v| if let DeviceFieldValue::Float(f) = v { Some(*f) } else { None })
            .unwrap_or(0.0);

        // Calculate flow rate
        let flow_rate = physics::add_noise(
            physics::calculate_flow_rate(upstream_pressure, valve_position),
            2.0
        );
        outputs.insert("flow_rate".to_string(), DeviceFieldValue::Float(flow_rate));

        // Accumulate volume if enabled
        if self.accumulate_volume {
            let total_volume = device.get_float("total_volume").unwrap_or(0.0);
            let new_volume = total_volume + flow_rate * dt / 60.0;  // Convert L/min to L
            outputs.insert("total_volume".to_string(), DeviceFieldValue::Float(new_volume));
        }

        // Update status based on flow rate
        let status = if flow_rate > 40.0 {
            "High"
        } else if flow_rate < 5.0 {
            "Low"
        } else {
            "Normal"
        };
        outputs.insert("status".to_string(), DeviceFieldValue::String(status.to_string()));

        outputs
    }
}

// ============================================================================
// Pressure Regulator Physics (ported from Valve::tick)
// ============================================================================

struct PressureRegulatorPhysics {
    target_pressure: f64,
    kp: f64,  // Proportional gain
}

impl PhysicsFunction for PressureRegulatorPhysics {
    fn compute(
        &self,
        device: &super::devices::Device,
        inputs: &HashMap<String, DeviceFieldValue>,
        _dt: f64,
    ) -> HashMap<String, DeviceFieldValue> {
        let mut outputs = HashMap::new();

        // Read upstream pressure (from devices.rs:217-238)
        let upstream_pressure = inputs.get("upstream_pressure")
            .and_then(|v| if let DeviceFieldValue::Float(f) = v { Some(*f) } else { None })
            .unwrap_or(0.0);

        let position = device.get_float("position").unwrap_or(0.5);

        // Auto-regulate based on pressure (proportional control)
        let tolerance = 0.5;
        let new_position = if upstream_pressure > self.target_pressure + tolerance {
            // Pressure too high, open valve more
            (position + self.kp).min(1.0)
        } else if upstream_pressure < self.target_pressure - tolerance {
            // Pressure too low, close valve
            (position - self.kp).max(0.0)
        } else {
            position
        };

        outputs.insert("position".to_string(), DeviceFieldValue::Float(new_position));

        // Update status based on position
        let status = if new_position > 0.8 {
            "Open"
        } else if new_position < 0.2 {
            "Closed"
        } else {
            "Partial"
        };
        outputs.insert("status".to_string(), DeviceFieldValue::String(status.to_string()));

        outputs
    }
}

// ============================================================================
// Static Physics (no automatic updates)
// ============================================================================

struct StaticPhysics;

impl PhysicsFunction for StaticPhysics {
    fn compute(
        &self,
        _device: &super::devices::Device,
        _inputs: &HashMap<String, DeviceFieldValue>,
        _dt: f64,
    ) -> HashMap<String, DeviceFieldValue> {
        // Static devices don't update automatically
        HashMap::new()
    }
}
