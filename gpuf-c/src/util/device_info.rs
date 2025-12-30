#[cfg(target_os = "macos")]
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct PowerMetrics {
    pub cpu_mw: u64,
    pub gpu_mw: u64,
    pub ane_mw: u64,
    pub total_mw: u64,
}

#[cfg(target_os = "macos")]
pub fn read_power_metrics() -> Option<PowerMetrics> {
    let mut child = Command::new("sudo")
        .arg("powermetrics")
        .arg("--samplers")
        .arg("cpu_power,gpu_power,ane_power")
        .arg("-n")
        .arg("1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let stdout = child.stdout.take()?;
    let reader = BufReader::new(stdout);

    let mut cpu_mw = 0.0;
    let mut gpu_mw = 0.0;
    let mut ane_mw = 0.0;

    for line in reader.lines().flatten() {
        if line.contains("CPU Power") {
            if let Some((_, value)) = parse_power_line(&line) {
                cpu_mw = value;
            }
        } else if line.contains("GPU Power") {
            if let Some((_, value)) = parse_power_line(&line) {
                gpu_mw = value;
            }
        } else if line.contains("ANE Power") {
            if let Some((_, value)) = parse_power_line(&line) {
                ane_mw = value;
            }
        }
    }

    // Manually terminate the process
    let _ = child.kill();

    Some(PowerMetrics {
        cpu_mw: cpu_mw as u64,
        gpu_mw: gpu_mw as u64,
        ane_mw: ane_mw as u64,
        total_mw: (cpu_mw + gpu_mw + ane_mw) as u64,
    })
}

#[allow(unused)]
fn parse_power_line(line: &str) -> Option<(String, f64)> {
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let name = parts[0].trim().to_string();
    if let Some(val_str) = parts[1].split_whitespace().next() {
        if let Ok(val) = val_str.parse::<f64>() {
            return Some((name, val));
        }
    }
    None
}

#[cfg(target_os = "macos")]
#[test]
fn test_read_power_metrics() {
    let metrics = read_power_metrics();
    println!("metrics: {:#?}", &metrics);
    assert!(metrics.is_some());
}
