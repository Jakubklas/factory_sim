use rand::Rng;

pub fn add_noise(value: f64, noise_percent: f64) -> f64 {
    let mut rng = rand::thread_rng();
    let noise = rng.gen_range(-noise_percent..noise_percent);
    value * (1.0 + noise / 100.0)
}

pub fn temperature_to_pressure(temperature: f64) -> f64 {
    // Simple linear approximation: 0°C = 0 bar, 100°C = 5 bar
    (temperature / 100.0) * 5.0
}

pub fn calculate_flow_rate(upstream_pressure: f64, valve_position: f64) -> f64 {
    // Flow rate proportional to pressure and valve opening
    upstream_pressure * valve_position * 10.0
}

pub fn pressure_decay(upstream_pressure: f64, flow_rate: f64, pipe_length: f64) -> f64 {
    // Simple pressure drop based on flow and distance
    let loss = flow_rate * pipe_length * 0.01;
    (upstream_pressure - loss).max(0.0)
}
