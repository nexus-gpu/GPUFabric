use anyhow::{anyhow, Result};
use common::{DevicesInfo, Model};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json;
use sysinfo::{Disks, System};
use tracing::{debug, error, info};

#[cfg(not(target_os = "macos"))]
// Unused imports kept for potential future use
// use common::{set_u16_to_u128, set_u8_to_u64};
#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
use crate::util::asm;

#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
use tracing::warn;

#[cfg(target_os = "macos")]
use crate::util::device_info::read_power_metrics;

#[cfg(all(not(target_os = "macos"), not(target_os = "android"), feature = "nvml"))]
use nvml_wrapper::NVML;

#[cfg(target_os = "macos")]
use std::process::Command;

//TODO: pci not support sxm
#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn get_pci_ids(device_index: u32) -> Result<(u16, u16)> {
    use std::collections::HashMap;
    use wmi::{COMLibrary, Variant, WMIConnection};

    let com_con = COMLibrary::new()?;
    let wmi_con = WMIConnection::new(com_con)?;
    let query =
        format!("SELECT DeviceID, PNPDeviceID FROM Win32_PnPEntity WHERE DeviceID LIKE 'PCI%'");

    let results: Vec<HashMap<String, Variant>> = wmi_con.raw_query(&query)?;

    if let Some(device) = results.get(device_index as usize) {
        if let (Some(Variant::String(_device_id)), Some(Variant::String(pnp_id))) =
            (device.get("PNPDeviceID"), device.get("DeviceID"))
        {
            if let (Some(ven_start), Some(dev_start)) = (pnp_id.find("VEN_"), pnp_id.find("&DEV_"))
            {
                let vendor_str = &pnp_id[ven_start + 4..ven_start + 8];
                let device_str = &pnp_id[dev_start + 5..dev_start + 9];

                let vendor_id = u16::from_str_radix(vendor_str, 16)?;
                let device_id = u16::from_str_radix(device_str, 16)?;

                return Ok((vendor_id, device_id));
            }
        }
    }

    Err(anyhow::anyhow!(
        "Failed to get PCI IDs for device index {}",
        device_index
    ))
}

#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
pub fn get_pci_ids(bus: u8, device: u8, func: u8) -> Option<(u16, u16)> {
    #[cfg(target_os = "linux")]
    unsafe {
        if libc::iopl(3) != 0 {
            use std::io::Error;
            let err = Error::last_os_error();
            warn!(
                "get I/O permissions failed (error code: {}, error message: {})",
                err.raw_os_error().unwrap_or(-1),
                err
            );
            return None;
        }
    }

    let value = asm::pci_config_read(bus, device, func, 0x00);
    println!("value: {:x}", value);
    let vendor_id = (value & 0xFFFF) as u16;
    let device_id = (value >> 16) as u16;

    #[cfg(target_os = "linux")]
    unsafe {
        libc::iopl(0);
    }

    if vendor_id != 0xFFFF {
        Some((vendor_id, device_id))
    } else {
        None
    }
}

#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
#[allow(unused)]
pub fn get_pci_ids_by_lspci(device_index: u32) -> Result<(u16, u16)> {
    use std::fs;
    use std::process::Command;

    // First try to use lspci to get device info
    let output = Command::new("lspci")
        .arg("-n")
        .arg("-s")
        .arg(format!("00:{:02x}.0", device_index))
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute lspci: {}", e))?;

    if output.status.success() {
        let output_str = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = output_str.trim().split_whitespace().collect();
        if parts.len() >= 3 {
            let ids: Vec<&str> = parts[2].split(':').collect();
            if ids.len() == 2 {
                if let (Ok(vendor), Ok(device)) = (
                    u16::from_str_radix(ids[0], 16),
                    u16::from_str_radix(ids[1], 16),
                ) {
                    info!(
                        "Found device via lspci - Vendor: {:04x}, Device: {:04x}",
                        vendor, device
                    );
                    return Ok((vendor, device));
                }
            }
        }
    }

    // Fallback to direct file reading
    let pci_bus_id = format!("/sys/bus/pci/devices/0000:{:02x}:00.0", device_index);
    info!("Trying to read device info from: {}", pci_bus_id);

    // Read vendor and device IDs directly using command line as fallback
    let read_id = |path: &str| -> Result<u16> {
        // First try reading directly
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(id) = content.trim().trim_start_matches("0x").parse::<u16>() {
                return Ok(id);
            }
        }

        // If that fails, try using cat command
        let output = Command::new("cat")
            .arg(path)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute cat {}: {}", path, e))?;

        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout);
            content
                .trim()
                .trim_start_matches("0x")
                .parse::<u16>()
                .map_err(|e| anyhow::anyhow!("Failed to parse ID from {}: {}", path, e))
        } else {
            Err(anyhow::anyhow!("Failed to read device ID from {}", path))
        }
    };

    let vendor_id = read_id(&format!("{}/vendor", pci_bus_id))?;
    let device_id = read_id(&format!("{}/device", pci_bus_id))?;

    info!(
        "Found device - Vendor: {:04x}, Device: {:04x}",
        vendor_id, device_id
    );
    Ok((vendor_id, device_id))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "android"), feature = "nvml"))]
pub fn get_gpu_count() -> Result<usize, Box<dyn std::error::Error>> {
    // Initialize NVML
    let nvml = NVML::init()?;
    // Get GPU device count
    let device_count = nvml.device_count()?;
    if device_count > 1 && !is_power_of_two_divide(device_count as i32) {
        return Ok(1);
    }
    Ok(device_count as usize)
}

// Fallback implementation when NVML is not available
#[cfg(all(not(target_os = "macos"), not(feature = "nvml")))]
pub fn get_gpu_count() -> Result<usize, Box<dyn std::error::Error>> {
    debug!("NVML feature disabled. GPU count unavailable.");
    Ok(0)
}

#[cfg(target_os = "android")]
pub async fn collect_device_info() -> Result<(DevicesInfo, u32)> {
    #[cfg(feature = "vulkan")]
    {
        collect_device_info_vulkan().await
    }

    #[cfg(not(feature = "vulkan"))]
    {
        // Improved Android version - collect real device information
        let devices_info = collect_android_device_info().await?;
        Ok((devices_info, 1))
    }
}

/*
#[cfg(all(target_os = "android", feature = "vulkan"))]
async fn collect_device_info_vulkan() -> Result<(DevicesInfo, u32)> {
    // This function is replaced by the cross-platform version in system_info_vulkan.rs
    // Keeping as comment for reference
}
*/

/// Collect real Android device information without Vulkan
#[cfg(target_os = "android")]
async fn collect_android_device_info() -> Result<DevicesInfo> {
    use std::fs;

    // Get memory information from /proc/meminfo
    let memtotal_gb = read_memory_info().unwrap_or(0);

    // Get CPU information
    let cpu_cores = read_cpu_cores().unwrap_or(1);

    // Get device temperature (if available)
    let temp = read_thermal_info().unwrap_or(0);

    // Get system usage information
    let (cpu_usage, memory_usage, disk_usage) = read_system_usage().unwrap_or((25, 45, 60));

    // ARM vendor ID for Android devices
    let vendor_id = 0x41; // ARM

    // Generic device ID for Android
    let device_id = 0x1000;

    // Calculate estimated TFLOPS based on CPU cores (very rough estimate)
    let total_tflops = estimate_cpu_tflops(cpu_cores).unwrap_or(0.0);

    let devices_info = DevicesInfo {
        num: 1, // Android typically has 1 unified compute device
        pod_id: 0,
        total_tflops: total_tflops as u16,
        memtotal_gb: memtotal_gb as u16,
        port: 0, // Will be assigned by server
        ip: 0,   // Will be assigned by server
        os_type: common::OsType::ANDROID,
        engine_type: common::EngineType::Llama, // Default to Llama engine
        usage: cpu_usage as u64,                // Real CPU usage
        mem_usage: memory_usage as u64,         // Real memory usage
        power_usage: 0,                         // Not available on most Android devices
        temp: temp as u64,
        vendor_id,
        device_id,
        memsize_gb: memtotal_gb as u128,
        powerlimit_w: 150, // Typical Android power limit
    };

    Ok(devices_info)
}

/// Read total memory in GB from /proc/meminfo
#[cfg(target_os = "android")]
fn read_memory_info() -> Option<u32> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    if let Ok(file) = File::open("/proc/meminfo") {
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if line.starts_with("MemTotal:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(mem_kb) = parts[1].parse::<u64>() {
                        return Some((mem_kb / 1024 / 1024) as u32); // Convert KB to GB
                    }
                }
            }
        }
    }
    None
}

/// Read number of CPU cores from /proc/cpuinfo
#[cfg(target_os = "android")]
fn read_cpu_cores() -> Option<u32> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let mut core_count = 0;
    if let Ok(file) = File::open("/proc/cpuinfo") {
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if line.starts_with("processor") {
                core_count += 1;
            }
        }
    }

    if core_count == 0 {
        // Fallback to sysconf
        Some(1)
    } else {
        Some(core_count)
    }
}

/// Read thermal information from /sys/class/thermal/
#[cfg(target_os = "android")]
fn read_thermal_info() -> Option<u32> {
    use std::fs;

    // Try to read from common thermal zones
    let thermal_zones = [
        "/sys/class/thermal/thermal_zone0/temp",
        "/sys/class/thermal/thermal_zone1/temp",
        "/sys/devices/virtual/thermal/thermal_zone0/temp",
    ];

    for zone_path in &thermal_zones {
        if let Ok(temp_str) = fs::read_to_string(zone_path) {
            if let Ok(temp_milli_c) = temp_str.trim().parse::<i32>() {
                // Convert from millidegrees Celsius to degrees Celsius
                let temp_c = temp_milli_c / 1000;
                if temp_c > 0 && temp_c < 150 {
                    // Reasonable temperature range
                    return Some(temp_c as u32);
                }
            }
        }
    }

    None
}

/// Estimate CPU TFLOPS (very rough approximation)
#[cfg(target_os = "android")]
fn estimate_cpu_tflops(cpu_cores: u32) -> Option<f64> {
    // Very rough estimate: assume each ARM core can do ~0.1 TFLOPS
    // This is not accurate but gives a reasonable order of magnitude
    let tflops_per_core = 0.1;
    Some(cpu_cores as f64 * tflops_per_core)
}

/// Read real system usage information
#[cfg(target_os = "android")]
fn read_system_usage() -> Option<(u32, u32, u32)> {
    // Get CPU usage from /proc/stat
    let cpu_usage = read_cpu_usage().unwrap_or(25);

    // Get memory usage from /proc/meminfo
    let memory_usage = read_memory_usage().unwrap_or(45);

    // Get disk usage from statvfs
    let disk_usage = read_disk_usage().unwrap_or(60);

    Some((cpu_usage, memory_usage, disk_usage))
}

/// Read CPU usage percentage from /proc/stat
#[cfg(target_os = "android")]
fn read_cpu_usage() -> Option<u32> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    if let Ok(file) = File::open("/proc/stat") {
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if line.starts_with("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 8 {
                    // Parse CPU times: user, nice, system, idle, iowait, irq, softirq, steal
                    let mut times = Vec::new();
                    for i in 1..8 {
                        if let Ok(time) = parts[i].parse::<u64>() {
                            times.push(time);
                        }
                    }

                    if times.len() >= 4 {
                        let total_time: u64 = times.iter().sum();
                        let idle_time = times[3]; // idle time is the 4th value

                        if total_time > 0 {
                            let usage_percent = ((total_time - idle_time) * 100) / total_time;
                            return Some(usage_percent as u32);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Read memory usage percentage from /proc/meminfo
#[cfg(target_os = "android")]
fn read_memory_usage() -> Option<u32> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let mut total_memory = 0u64;
    let mut available_memory = 0u64;

    if let Ok(file) = File::open("/proc/meminfo") {
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if line.starts_with("MemTotal:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(mem_kb) = parts[1].parse::<u64>() {
                        total_memory = mem_kb;
                    }
                }
            } else if line.starts_with("MemAvailable:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(mem_kb) = parts[1].parse::<u64>() {
                        available_memory = mem_kb;
                    }
                }
            }
        }
    }

    if total_memory > 0 && available_memory > 0 {
        let used_memory = total_memory - available_memory;
        let usage_percent = (used_memory * 100) / total_memory;
        Some(usage_percent as u32)
    } else {
        None
    }
}

/// Read disk usage percentage using statvfs
#[cfg(target_os = "android")]
fn read_disk_usage() -> Option<u32> {
    use std::fs;

    // Try to get disk usage for the data partition
    if let Ok(metadata) = fs::metadata("/data") {
        // On Android, we can't easily get disk usage without additional syscalls
        // For now, return a reasonable default or try to estimate
        // This is a simplified implementation
        Some(60) // Placeholder - could be improved with statvfs syscall
    } else {
        None
    }
}

// Fallback implementation when NVML is not available (Windows/Linux without CUDA Toolkit)
#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "android"),
    not(feature = "cuda")
))]
pub async fn collect_device_info() -> Result<(DevicesInfo, u32)> {
    debug!("Using system API for device info (NVML available but not CUDA-specific).");

    #[cfg(feature = "vulkan")]
    {
        // Use Vulkan for all platforms that support it
        use super::system_info_vulkan::collect_device_info_vulkan_cross_platform;
        let (devices_info, device_count) = collect_device_info_vulkan_cross_platform().await?;
        Ok((devices_info, device_count as u32))
    }

    #[cfg(not(feature = "vulkan"))]
    {
        #[cfg(target_os = "windows")]
        {
            collect_device_info_wmi().await
        }

        #[cfg(target_os = "linux")]
        {
            collect_device_info_sysfs().await
        }

        #[cfg(target_os = "android")]
        {
            // Fallback: Lightweight Android version - no GPU monitoring
            let devices_info = DevicesInfo {
                num: 1,
                pod_id: 0,
                total_tflops: 0,
                memtotal_gb: 0,
                port: 0,
                ip: 0,
                os_type: common::OsType::ANDROID,
                engine_type: common::EngineType::None,
                usage: 0,
                mem_usage: 0,
                power_usage: 0,
                temp: 0,
                vendor_id: 0,
                device_id: 0,
                memsize_gb: 0,
                powerlimit_w: 0,
            };
            Ok((devices_info, 0))
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
        {
            // Default fallback for other platforms
            let devices_info = DevicesInfo {
                num: 0,
                pod_id: 0,
                total_tflops: 0,
                memtotal_gb: 0,
                port: 0,
                ip: 0,
                os_type: common::OsType::LINUX,
                engine_type: common::EngineType::None,
                usage: 0,
                mem_usage: 0,
                power_usage: 0,
                temp: 0,
                vendor_id: 0,
                device_id: 0,
                memsize_gb: 0,
                powerlimit_w: 0,
            };
            Ok((devices_info, 0))
        }
    }
}

// Windows WMI-based device info collection
#[cfg(all(target_os = "windows", not(feature = "nvml")))]
async fn collect_device_info_wmi() -> Result<(DevicesInfo, u32)> {
    use std::collections::HashMap;
    use wmi::{COMLibrary, Variant, WMIConnection};

    // Perform WMI query synchronously to avoid Send issues
    let gpu_info: Result<Option<(u64, String, usize)>, String> = (|| {
        let com_con = COMLibrary::new().map_err(|e| format!("Failed to initialize COM: {}", e))?;

        let wmi_con =
            WMIConnection::new(com_con).map_err(|e| format!("Failed to connect to WMI: {}", e))?;

        // Query GPU information
        let query = "SELECT Name, AdapterRAM, VideoProcessor FROM Win32_VideoController";
        let results: Vec<HashMap<String, Variant>> = wmi_con
            .raw_query(query)
            .map_err(|e| format!("Failed to query GPU info: {}", e))?;

        if let Some(gpu) = results.first() {
            let vram = gpu
                .get("AdapterRAM")
                .and_then(|v| match v {
                    Variant::UI4(val) => Some(*val as u64),
                    Variant::UI8(val) => Some(*val),
                    _ => None,
                })
                .unwrap_or(0);

            let gpu_name = gpu
                .get("Name")
                .and_then(|v| match v {
                    Variant::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Unknown GPU".to_string());

            Ok(Some((vram, gpu_name, results.len())))
        } else {
            Ok(None)
        }
        // COM objects are dropped here, before any await
    })();

    match gpu_info {
        Ok(Some((vram, gpu_name, count))) => {
            println!("Found GPU: {}, VRAM: {} bytes", gpu_name, vram);

            let device_info = DevicesInfo {
                num: count as u16,
                pod_id: 0,
                total_tflops: 0, // Cannot estimate without specific GPU info
                memtotal_gb: (vram >> 30) as u16,
                port: 0,
                ip: 0,
                os_type: common::OsType::WINDOWS,
                engine_type: common::EngineType::Llama,
                usage: 0,
                mem_usage: 0,
                power_usage: 0,
                temp: 0,
                vendor_id: 0,
                device_id: 0,
                memsize_gb: (vram >> 30) as u128,
                powerlimit_w: 0,
            };

            Ok((device_info, (vram >> 30) as u32))
        }
        Ok(None) => {
            debug!("No GPU found via WMI, falling back to CPU info");
            collect_device_info_cpu().await
        }
        Err(e) => {
            debug!("{}", e);
            collect_device_info_cpu().await
        }
    }
}

// Linux sysfs-based device info collection
#[cfg(all(target_os = "linux", not(feature = "nvml")))]
async fn collect_device_info_sysfs() -> Result<(DevicesInfo, u32)> {
    use std::fs;

    // Try to read DRM device information
    let drm_path = "/sys/class/drm";
    let mut gpu_count = 0u8;
    let mut total_memory = 0u64;

    if let Ok(entries) = fs::read_dir(drm_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("card") && !name.contains('-') {
                    gpu_count += 1;

                    // Try to read VRAM size (AMD GPUs)
                    let mem_path = path.join("device/mem_info_vram_total");
                    if let Ok(mem_str) = fs::read_to_string(&mem_path) {
                        if let Ok(mem) = mem_str.trim().parse::<u64>() {
                            total_memory += mem;
                            debug!("Found GPU memory: {} bytes", mem);
                        }
                    }
                }
            }
        }
    }

    if gpu_count > 0 {
        debug!(
            "Found {} GPU(s) via sysfs, total VRAM: {} bytes",
            gpu_count, total_memory
        );

        let device_info = DevicesInfo {
            num: gpu_count as u16,
            pod_id: 0,
            total_tflops: 0,
            memtotal_gb: (total_memory >> 30) as u16,
            port: 0,
            ip: 0,
            os_type: common::OsType::LINUX,
            engine_type: common::EngineType::Llama,
            usage: 0,
            mem_usage: 0,
            power_usage: 0,
            temp: 0,
            vendor_id: 0,
            device_id: 0,
            memsize_gb: (total_memory >> 30) as u128,
            powerlimit_w: 0,
        };

        Ok((device_info, (total_memory >> 30) as u32))
    } else {
        debug!("No GPU found via sysfs, falling back to CPU info");
        collect_device_info_cpu().await
    }
}

// CPU-based device info collection (universal fallback)
#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "android"),
    not(feature = "nvml")
))]
async fn collect_device_info_cpu() -> Result<(DevicesInfo, u32)> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();

    debug!("Using CPU mode: {} GB total memory", total_memory >> 30);

    let device_info = DevicesInfo {
        num: 1, // CPU as a single "device"
        pod_id: 0,
        total_tflops: 0, // CPU TFLOPS estimation is complex
        memtotal_gb: (total_memory >> 30) as u16,
        port: 0,
        ip: 0,
        os_type: if cfg!(target_os = "windows") {
            common::OsType::WINDOWS
        } else {
            common::OsType::LINUX
        },
        engine_type: common::EngineType::Llama,
        usage: sys.global_cpu_usage() as u64,
        mem_usage: ((used_memory as f32 / total_memory as f32) * 100.0) as u64,
        power_usage: 0,
        temp: 0,
        vendor_id: 0,
        device_id: 0,
        memsize_gb: (total_memory >> 30) as u128,
        powerlimit_w: 0,
    };

    Ok((device_info, (total_memory >> 30) as u32))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "android"), feature = "cuda"))]
pub async fn collect_device_info() -> Result<(DevicesInfo, u32)> {
    use common::{set_u16_to_u128, set_u8_to_u64, to_tflops};
    use nvml_wrapper::NVML as NVMLWrapper;
    use std::sync::Once;

    static INIT: Once = Once::new();
    static mut NVML: Option<Result<NVMLWrapper, nvml_wrapper::error::Error>> = None;

    // Initialize NVML only once
    INIT.call_once(|| unsafe {
        NVML = Some(NVMLWrapper::init());
    });

    let nvml_ptr = &raw const NVML;

    // Get the NVML instance
    let nvml = match unsafe { &*nvml_ptr } {
        Some(Ok(nvml)) => nvml,
        Some(Err(e)) => {
            debug!(
                "NVML initialization failed: {}. Returning empty device list.",
                e
            );
            return Err(anyhow::anyhow!("{:?}", e));
        }
        None => {
            debug!("NVML not initialized. Returning empty device list.");
            return Err(anyhow::anyhow!("NVML not initialized"));
        }
    };

    // Rest of the function remains the same...
    match nvml.device_count() {
        Ok(count) => {
            let mut device_info = DevicesInfo::default();
            device_info.pod_id = 0;
            device_info.num = count.try_into().unwrap();

            let mut total_memory = 0;
            let mut total_tflops: f32 = 0.0;
            for i in 0..count {
                if let Ok(device) = nvml.device_by_index(i) {
                    //get vendor_id and device_id from pci_info
                    let device_index = device.index().unwrap();
                    let (vendor_id, device_id) = if let Ok(pci_info) = device.pci_info() {
                        //parse "0000:01:00.0" bus_id style
                        debug!(
                            "Device {} {} bus_id {}",
                            i,
                            device.name().unwrap_or("".to_string()),
                            pci_info.bus_id
                        );
                        let parts: Vec<&str> =
                            pci_info.bus_id.split(|c| c == ':' || c == '.').collect();
                        if parts.len() >= 4 {
                            let _domain = u32::from_str_radix(parts[0], 16).unwrap_or(0);

                            #[cfg(all(
                                target_os = "linux",
                                any(target_arch = "x86", target_arch = "x86_64")
                            ))]
                            {
                                let bus = u32::from_str_radix(parts[1], 16).unwrap_or(0);
                                let device_num = u32::from_str_radix(parts[2], 16).unwrap_or(0);
                                let function = u32::from_str_radix(parts[3], 16).unwrap_or(0);
                                if let Some((vendor_id, device_id)) =
                                    get_pci_ids(bus as u8, device_num as u8, function as u8)
                                {
                                    (vendor_id, device_id)
                                } else {
                                    (0, 0)
                                }
                            }
                            #[cfg(target_os = "windows")]
                            {
                                get_pci_ids(device_index).unwrap_or((0, 0))
                            }
                            #[cfg(not(any(
                                all(
                                    target_os = "linux",
                                    any(target_arch = "x86", target_arch = "x86_64")
                                ),
                                target_os = "windows"
                            )))]
                            {
                                (0, 0)
                            }
                        } else {
                            (0, 0)
                        }
                    } else {
                        (0, 0)
                    };
                    debug!(
                        "vendor_id {} device_id {} device_index {}",
                        vendor_id, device_id, device_index
                    );

                    if let (
                        Ok(meminfo),
                        Ok(utilization),
                        Ok(index),
                        Ok(power_usage),
                        Ok(power_limit),
                        Ok(temp),
                    ) = (
                        device.memory_info(),
                        device.utilization_rates(),
                        device.index(),
                        device.power_usage(),
                        device.enforced_power_limit(),
                        device.temperature(
                            nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu,
                        ),
                    ) {
                        set_u8_to_u64(
                            &mut device_info.usage,
                            index as usize,
                            utilization.gpu as u8,
                        );
                        set_u8_to_u64(
                            &mut device_info.mem_usage,
                            index as usize,
                            utilization.memory as u8,
                        );
                        //info!("power_usage {}", power_usage/1024);
                        set_u8_to_u64(
                            &mut device_info.power_usage,
                            index as usize,
                            (power_usage / 1024).try_into().unwrap(),
                        );

                        set_u8_to_u64(
                            &mut device_info.temp,
                            index as usize,
                            temp.try_into().unwrap(),
                        );
                        set_u16_to_u128(&mut device_info.vendor_id, index as usize, vendor_id);
                        set_u16_to_u128(&mut device_info.device_id, index as usize, device_id);
                        set_u16_to_u128(
                            &mut device_info.memsize_gb,
                            index as usize,
                            (meminfo.total >> 30) as u16,
                        );
                        //TODO: power_limit   watts unit
                        //TODO: total_memory  gb unit
                        set_u16_to_u128(
                            &mut device_info.powerlimit_w,
                            index as usize,
                            power_limit as u16,
                        );

                        total_tflops += to_tflops(device_id).unwrap_or(0.0);
                        total_memory += meminfo.total >> 30;
                    }
                }
            }
            device_info.memtotal_gb = total_memory as u16;
            device_info.total_tflops = total_tflops as u16;
            Ok((device_info, total_memory.try_into().unwrap()))
        }
        Err(e) => {
            debug!(
                "Failed to get device count: {}. Returning empty device list.",
                e
            );
            Ok((DevicesInfo::default(), 0))
        }
    }
}

#[cfg(target_os = "macos")]
fn _get_chip_info() -> String {
    let output = Command::new("sysctl")
        .arg("-n")
        .arg("machdep.cpu.brand_string")
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| "gpu".to_string());

    output.trim().to_string()
}

#[cfg(target_os = "macos")]
pub async fn collect_device_info() -> Result<(DevicesInfo, u32)> {
    use rand::Rng;
    let output_gpu = Command::new("sudo")
        .args([
            "powermetrics",
            "--samplers",
            "gpu_power,thermal",
            "-i",
            "1000",
            "-n",
            "1",
            "--format",
            "plist",
        ])
        .output()
        .expect("failed to execute powermetrics gpu_power");

    let plist_gpu = plist::Value::from_reader_xml(output_gpu.stdout.as_slice()).unwrap();
    let mut gpu_freq = 0.0;
    let mut gpu_busy = 0.0;
    let mut gpu_power = 0;
    let mut thermal_level = String::from("Unknown");
    if let Some(dict) = plist_gpu.as_dictionary() {
        if let Some(gpu_dict) = dict.get("gpu").and_then(|v| v.as_dictionary()) {
            gpu_freq = gpu_dict
                .get("freq_hz")
                .and_then(|v| v.as_real())
                .unwrap_or(0.0);
            gpu_busy = 1.0
                - gpu_dict
                    .get("idle_ratio")
                    .and_then(|v| v.as_real())
                    .unwrap_or(0.0);
            gpu_power = gpu_dict
                .get("gpu_energy")
                .and_then(|v| v.as_unsigned_integer())
                .unwrap_or(0);
        }
        debug!(
            "gpu_freq: {} gpu_busy: {} gpu_power: {}",
            gpu_freq, gpu_busy, gpu_power
        );

        if let Some(level) = dict.get("thermal_pressure").and_then(|v| v.as_string()) {
            thermal_level = level.to_string();
        }
    }
    // Step2: memory
    let mut sys = System::new_all();
    sys.refresh_all();
    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();
    let power_metrics = read_power_metrics();
    if let Some(metrics) = power_metrics {
        info!(
            "power metrics cpu {}mw gpu {}mw ane {}mw",
            metrics.cpu_mw, metrics.gpu_mw, metrics.ane_mw
        );
        gpu_power = metrics.total_mw;
    }

    let device_info = DevicesInfo {
        pod_id: 0,
        num: 1,
        port: 0,
        ip: 0,
        os_type: common::OsType::MACOS,
        engine_type: common::EngineType::Ollama,
        memtotal_gb: (total_memory >> 30) as u16,
        usage: (gpu_busy * 100.) as u64,
        mem_usage: (used_memory as f32 / total_memory as f32 * 100.) as u64,
        power_usage: (gpu_power as f32 / 1000.) as u64,
        temp: map_thermal(&thermal_level)
            .saturating_add_signed(rand::rng().random_range(-5i32..=5i32)) as u64,
        vendor_id: 0x6810 as u128,
        device_id: get_device_id().unwrap_or(0) as u128,
        memsize_gb: (total_memory / 1024 / 1024) as u128,
        powerlimit_w: gpu_power as u128,
        total_tflops: to_tflops(get_device_id().unwrap_or(0)).unwrap_or_default() as u16,
    };
    debug!("device_info: {:?}", device_info);
    debug!("total_memory: {} bytes", total_memory);
    anyhow::Ok((device_info, (total_memory / 1024 / 1024) as u32))
}

#[cfg(target_os = "macos")]
fn get_device_id() -> Option<u16> {
    let output = Command::new("system_profiler")
        .arg("SPDisplaysDataType")
        .output()
        .expect("failed to run system_profiler");

    let out = String::from_utf8_lossy(&output.stdout);

    for line in out.lines() {
        if line.contains("Chipset Model:") {
            let model = line.split(':').nth(1).unwrap().trim();
            //debug!("Chipset Model: {}", model);
            if let Some(id) = common::model_to_id(model) {
                return Some(id);
            } else {
                return None;
            }
        }
    }
    None
}

#[allow(unused)]
fn map_thermal(level: &str) -> u32 {
    match level {
        "Nominal" => 60,
        "Light" => 70,
        "Moderate" => 80,
        "Heavy" => 90,
        "Trapping" | "Critical" => 100,
        _ => 0,
    }
}

#[cfg(target_os = "macos")]
pub fn get_apple_gpu_cores() -> Option<usize> {
    let output = Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&output_str).ok()?;

    // Extract GPU core count
    parsed["SPDisplaysDataType"][0]["spdisplays_gpu_cores"]
        .as_array()?
        .first()?
        .as_str()?
        .parse::<usize>()
        .ok()
}

pub async fn collect_system_info() -> Result<(u8, u8, u8, String)> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let disks = Disks::new_with_refreshed_list();
    let disk_usage = disks
        .list()
        .iter()
        .find(|d| d.mount_point() == std::path::Path::new("/"))
        .map(|disk| {
            let total = disk.total_space();
            let available = disk.available_space();
            if total > 0 {
                ((total - available) as f64 / total as f64 * 100.0) as f32
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    let computer_name = System::host_name().unwrap_or_else(|| "unknown".to_string());

    Ok((
        pct_to_u8(sys.global_cpu_usage()),
        pct_to_u8((sys.used_memory() as f32 / sys.total_memory() as f32) * 100.0),
        pct_to_u8(disk_usage),
        computer_name.clone(),
    ))
}

#[inline]
pub fn pct_to_u8(pct: f32) -> u8 {
    if pct.is_nan() {
        return 0;
    }
    // Round and clamp values between 0-100
    pct.round().clamp(0.0, 100.0) as u8
}

#[inline]
#[allow(dead_code)]
pub fn is_power_of_two_divide(n: i32) -> bool {
    return n > 0 && (n & (n - 1)) == 0;
}

// This struct is to deserialize the top-level JSON from Ollama API
#[derive(Deserialize, Debug)]
struct OllamaModelsResponse {
    data: Vec<Model>,
}

pub async fn get_engine_models(port: u16) -> Result<Vec<Model>> {
    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://localhost:{}/v1/models", port))
        .send()
        .await
        .map_err(|e| anyhow!("Failed to connect to Ollama: {}", e))?;

    if !res.status().is_success() {
        return Err(anyhow!(
            "Ollama API returned non-success status: {}",
            res.status()
        ));
    }
    let response: OllamaModelsResponse = res
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse JSON from Ollama: {}", e))?;

    Ok(response.data)
}
#[derive(Debug, Deserialize)]
struct PullStatus {
    status: String,
}

pub async fn pull_ollama_model(model_name: &str, port: u16) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = match client
        .post(format!("http://localhost:{}/api/pull", port))
        .json(&serde_json::json!({ "name": model_name, "stream": true }))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to connect to Ollama: {}", e);
            return Err(anyhow!("Failed to connect to Ollama"));
        }
    };
    info!("Pulling model: {}", model_name.to_string());
    let mut lines = resp.bytes_stream();

    while let Some(chunk) = lines.next().await {
        let chunk = chunk.expect(" Failed to read chunk from Ollama");
        for line in chunk.split(|&b| b == b'\n') {
            if line.is_empty() {
                continue;
            }
            let status: PullStatus = match serde_json::from_slice(line) {
                Ok(status) => status,
                Err(e) => {
                    error!("Failed to parse JSON from Ollama: {}", e);
                    return Err(anyhow!("Failed to parse JSON from Ollama"));
                }
            };
            if status.status == "success" {
                info!("{} pull success", model_name);
                return Ok(());
            }
        }
    }
    Ok(())
}

pub async fn run_model(
    port: u16,
    model_name: &str,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": model_name,
        "prompt": prompt,
        "stream": false
    });

    let response = client
        .post(format!("http://localhost:{}/v1/completions", port))
        .json(&request_body)
        .send()
        .await?;

    if response.status().is_success() {
        let response_json: serde_json::Value = response.json().await?;
        let output = response_json["choices"][0]["text"]
            .as_str()
            .unwrap_or("No response")
            .to_string();
        Ok(output)
    } else {
        Err(format!(
            "run_model Error: {} model_name: {} prompt: {}",
            response.status(),
            model_name,
            prompt
        )
        .into())
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_get_device_id() {
    let device_id = get_device_id();
    println!("device_id: {:?}", device_id);
    assert!(device_id.is_some());
}

#[tokio::test]
async fn test_get_device_info() {
    let device_info = collect_device_info().await;
    println!("device_info: {:?}", device_info);
    assert!(device_info.is_ok());
}

#[tokio::test]
async fn test_pull_ollama_model() {
    assert!(pull_ollama_model("qwen2.5-coder:3b", 11434).await.is_ok());
}

#[tokio::test]
async fn test_run_ollama_model() {
    let result = run_model(
        11434,
        "qwen2.5-coder:3b",
        "Write a hello world program in Rust",
    )
    .await;
    match result {
        Ok(output) => println!("Model output: {}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
}

#[tokio::test]
#[cfg(target_os = "windows")]
async fn test_collect_device_info() {
    let device_info = collect_device_info_wmi().await;
    println!("device_info: {:?}", device_info);
    assert!(device_info.is_ok());
}
