// Ensures TCP ports are free before the simulator binds to them.
// Finds and kills any process holding a given port via lsof.
// No-ops gracefully if lsof is unavailable (non-Unix environments).

/// Kill any process currently holding the given TCP ports.
/// Called at startup and after shutdown to leave ports clean for the next run.
pub fn release_ports(ports: &[u16]) {
    for &port in ports {
        let pids_output = std::process::Command::new("lsof")
            .args(["-ti", &format!("TCP:{}", port)])
            .output();

        match pids_output {
            Err(e) => {
                tracing::debug!("lsof unavailable, skipping port {} cleanup: {}", port, e);
            }
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for pid_str in stdout.split_whitespace() {
                    if let Ok(pid) = pid_str.trim().parse::<u32>() {
                        // Never kill ourselves
                        if pid == std::process::id() { continue; }
                        let killed = std::process::Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false);
                        if killed {
                            tracing::info!("Released port {} (killed PID {})", port, pid);
                        }
                    }
                }
            }
        }
    }
}
