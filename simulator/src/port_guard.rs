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
