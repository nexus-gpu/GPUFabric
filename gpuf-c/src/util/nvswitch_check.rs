#[cfg(feature = "cuda")]
use nvml_wrapper::NVML;
#[cfg(feature = "cuda")]
use tracing::info;
use tracing::warn;

/// Check if running on multi-GPU HGX/DGX system (8x A100/A800)
/// Simplified: detect if more than 4 GPUs present (indicates HGX)
#[cfg(feature = "cuda")]
pub fn check_nvswitch_topology() -> Result<bool, Box<dyn std::error::Error>> {
    let nvml = NVML::init()?;
    let device_count = nvml.device_count()?;

    if device_count <= 4 {
        // Less than 4 GPUs: regular workstation/server, no NVSwitch
        return Ok(false);
    }

    // Check GPU names to confirm A100/A800/H100/H100 (datacenter GPUs with NVSwitch)
    let mut is_hgx = false;
    for i in 0..device_count.min(2) {
        if let Ok(device) = nvml.device_by_index(i) {
            if let Ok(name) = device.name() {
                let name_lower = name.to_lowercase();
                if name_lower.contains("a100")
                    || name_lower.contains("a800")
                    || name_lower.contains("h100")
                    || name_lower.contains("h800")
                {
                    is_hgx = true;
                    info!("Detected datacenter GPU: {}", name);
                    break;
                }
            }
        }
    }

    // Only report NVSwitch if datacenter GPUs detected (not just 8+ consumer GPUs)
    if is_hgx {
        info!(
            "Detected {} datacenter GPUs, NVSwitch fabric present",
            device_count
        );
        return Ok(true);
    }

    Ok(false)
}

/// Check if running on multi-GPU HGX/DGX system (8x A100/A800)
/// Simplified: detect if more than 4 GPUs present (indicates HGX)
#[cfg(not(feature = "cuda"))]
pub fn check_nvswitch_topology() -> Result<bool, Box<dyn std::error::Error>> {
    Ok(false)
}

/// Check if Fabric Manager service is active (HGX required)
pub fn check_fabric_manager_service() -> bool {
    match crate::util::safe_command::run_command_default(
        "systemctl",
        &["is-active", "nvidia-fabricmanager"],
    ) {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Full HGX/NVSwitch availability check
pub fn check_hgx_nvswitch_available() -> bool {
    // 1. Check if we're on HGX/NVSwitch topology
    let has_nvswitch = match check_nvswitch_topology() {
        Ok(v) => v,
        Err(e) => {
            warn!("NVML topology check failed: {}", e);
            false
        }
    };

    if !has_nvswitch {
        // Not HGX/DGX, regular GPU setup - no Fabric Manager needed
        return true;
    }

    // 2. HGX detected - Fabric Manager must be running
    let fm_active = check_fabric_manager_service();

    if !fm_active {
        warn!("NVSwitch/HGX detected but Fabric Manager not running!");
        warn!("CUDA will fail with error 802 (cudaErrorSystemNotReady)");
        warn!("Fix: sudo systemctl start nvidia-fabricmanager");
    }

    fm_active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_check() {
        match check_nvswitch_topology() {
            Ok(has_nvswitch) => {
                println!("NVSwitch detected: {}", has_nvswitch);
            }
            Err(e) => {
                println!("Topology check failed (expected if no GPU): {}", e);
            }
        }
    }

    #[test]
    fn test_fabric_manager_check() {
        let active = check_fabric_manager_service();
        println!("Fabric Manager active: {}", active);
    }
}
