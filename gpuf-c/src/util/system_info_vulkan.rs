//! Cross-platform Vulkan device information collection module

#[cfg(feature = "vulkan")]
use anyhow::Result;
#[cfg(feature = "vulkan")]
use ash::{vk, Entry};
#[cfg(feature = "vulkan")]
use common::{DevicesInfo, OsType, EngineType};
#[cfg(feature = "vulkan")]
use sysinfo;

// Conditional debug printing: only print in debug builds
macro_rules! debug_println {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        println!($($arg)*);
    };
}

#[cfg(feature = "vulkan")]
pub async fn collect_device_info_vulkan_cross_platform() -> Result<(DevicesInfo, u16)> {
    #[allow(unused)]
    let platform_name = if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "android") {
        "Android"
    } else {
        "Unknown"
    };
    
    debug_println!("Starting Vulkan API device info collection for {}...", platform_name);
    
    // Initialize Vulkan
    let entry = unsafe { Entry::load() }
        .map_err(|e| anyhow::anyhow!("Failed to load Vulkan entry: {}", e))?;
    
    debug_println!("Vulkan entry point loaded successfully");
    
    // Create instance
    let app_info = vk::ApplicationInfo::builder()
        .api_version(vk::API_VERSION_1_0);
    
    let create_info = vk::InstanceCreateInfo::builder()
        .application_info(&app_info);
    
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(|e| anyhow::anyhow!("Failed to create Vulkan instance: {}", e))?;
    
    debug_println!("Vulkan instance created successfully");
    
    // Enumerate physical devices
    let physical_devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| anyhow::anyhow!("Failed to enumerate physical devices: {}", e))?;
    
    debug_println!("Found {} Vulkan physical devices", physical_devices.len());
    
    let mut total_tflops = 0u16;
    let mut total_memory_gb = 0u32;
    let mut device_count = 0u16;
    let mut gpu_details = Vec::new();
    let mut first_vendor_id = 0u128;
    let mut first_device_id = 0u128;
    
    for (index, &physical_device) in physical_devices.iter().enumerate() {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical_device) };
        let features = unsafe { instance.get_physical_device_features(physical_device) };
        let queue_families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        
        // Get device name
        let device_name = unsafe {
            std::ffi::CStr::from_ptr(properties.device_name.as_ptr())
                .to_string_lossy()
        };

        let is_software_or_cpu = {
            let name_lc = device_name.to_ascii_lowercase();
            properties.device_type == vk::PhysicalDeviceType::CPU
                || name_lc.contains("llvmpipe")
                || name_lc.contains("lavapipe")
        };
        
        // Calculate device memory
        let mut device_memory = 0u64;
        let mut heap_count = 0;
        for (i, heap) in memory_properties.memory_heaps.iter().enumerate() {
            debug_println!("  Memory heap {}: {}MB, flags: {:?}", 
                i, heap.size / (1024 * 1024), heap.flags);
            if heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
                device_memory += heap.size;
                heap_count += 1;
            }
        }
        let device_memory_gb = (device_memory / (1024 * 1024 * 1024)) as u32;
        if !is_software_or_cpu {
            total_memory_gb += device_memory_gb;
        }
        
        // Estimate TFLOPS based on device type and vendor
        let tflops = if is_software_or_cpu {
            0
        } else {
            estimate_gpu_tflops_cross_platform(properties.vendor_id, properties.device_id, device_name.as_bytes())
        };
        if !is_software_or_cpu {
            total_tflops = total_tflops.saturating_add(tflops);
        }
        
        // Count queue families
        let mut graphics_queues = 0;
        let mut compute_queues = 0;
        let mut transfer_queues = 0;
        
        for queue_family in &queue_families {
            if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                graphics_queues += queue_family.queue_count;
            }
            if queue_family.queue_flags.contains(vk::QueueFlags::COMPUTE) {
                compute_queues += queue_family.queue_count;
            }
            if queue_family.queue_flags.contains(vk::QueueFlags::TRANSFER) {
                transfer_queues += queue_family.queue_count;
            }
        }
        
        // Print detailed GPU information
        debug_println!("\nGPU {} Details:", index + 1);
        debug_println!("  Device Name: {}", device_name);
        debug_println!("  Device Type: {:?}", properties.device_type);
        if is_software_or_cpu {
            debug_println!("  Note: software/CPU Vulkan device (not a real GPU)");
        }
        debug_println!("  Vendor ID: 0x{:04x}", properties.vendor_id);
        debug_println!("  Device ID: 0x{:04x}", properties.device_id);
        debug_println!("  Device Memory: {}GB ({} heaps)", device_memory_gb, heap_count);
        debug_println!("  Performance Estimate: {} TFLOPS", tflops);
        debug_println!("  API Version: {}.{}.{}", 
            vk::api_version_major(properties.api_version),
            vk::api_version_minor(properties.api_version),
            vk::api_version_patch(properties.api_version)
        );
        debug_println!("  Queue Count: Graphics{} Compute{} Transfer{}", graphics_queues, compute_queues, transfer_queues);
        debug_println!("  Supported Features:");
        debug_println!("    - Geometry Shader: {}", features.geometry_shader != 0);
        debug_println!("    - Tessellation Shader: {}", features.tessellation_shader != 0);
        debug_println!("    - Multi Viewport: {}", features.multi_viewport != 0);
        debug_println!("    - Shader Storage: {}", features.shader_storage_image_extended_formats != 0);
        
        // Store GPU details for summary
        if !is_software_or_cpu {
            gpu_details.push(format!("GPU{}: {} ({}GB, {}TFLOPS)", 
                index + 1, device_name, device_memory_gb, tflops));
        }
        
        // Store first device info for DevicesInfo
        if !is_software_or_cpu && first_vendor_id == 0 {
            first_vendor_id = properties.vendor_id as u128;
            first_device_id = properties.device_id as u128;
        }
        
        if !is_software_or_cpu {
            device_count = device_count.saturating_add(1);
        }
    }
    
    // Get system information
    let mut system = sysinfo::System::new();
    system.refresh_all();
    let system_memory_gb = (system.total_memory() / (1024 * 1024 * 1024)) as u16;
    let used_memory = system.used_memory();
    let total_memory = system.total_memory();
    
    // Get CPU info
    let cpu_brand = system.cpus().first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    
    // Try to get accurate GPU metrics from platform-specific APIs
    let (gpu_usage, gpu_mem_usage, power_usage, temp) = 
        get_accurate_gpu_metrics(first_vendor_id, device_count, total_tflops, total_memory_gb);
    
    debug_println!("\nSystem Information Summary:");
    debug_println!("  CPU: {}", cpu_brand);
    debug_println!("  System Memory: {}GB", system_memory_gb);
    debug_println!("  System Memory Usage: {}% ({}GB/{}GB)", ((used_memory as f32 / total_memory as f32) * 100.0) as u64, used_memory / (1024*1024*1024), total_memory / (1024*1024*1024));
    debug_println!("  CPU Usage: {}%", system.global_cpu_usage());
    debug_println!("  GPU Count: {}", device_count);
    debug_println!("  Total Compute Power: {} TFLOPS", total_tflops);
    
    // Show GPU metrics with accuracy indication
    if gpu_usage > 0 || gpu_mem_usage > 0 {
        debug_println!("  GPU Usage: {}%", gpu_usage);
        debug_println!("  GPU Memory Usage: {}%", gpu_mem_usage);
        debug_println!("  GPU Power: {}W", power_usage);
        debug_println!("  GPU Temperature: {}°C", temp);
    } else {
        debug_println!("  GPU Usage (estimated): {}%", gpu_usage);
        debug_println!("  GPU Memory Usage (estimated): {}%", gpu_mem_usage);
        debug_println!("  GPU Power (estimated): {}W", power_usage);
        debug_println!("  GPU Temperature (estimated): {}°C", temp);
    }
    debug_println!("  Operating System: {}", platform_name);
    
    if device_count > 0 {
        debug_println!("  GPU List: {}", gpu_details.join(", "));
    }

    
    // Determine OS type
    let os_type = if cfg!(target_os = "windows") {
        OsType::WINDOWS
    } else if cfg!(target_os = "linux") {
        OsType::LINUX
    } else if cfg!(target_os = "android") {
        OsType::ANDROID
    } else {
        OsType::LINUX // Default
    };
    
    let devices_info = DevicesInfo {
        num: device_count,
        pod_id: 0,
        total_tflops,
        memtotal_gb: system_memory_gb,
        port: 0,
        ip: 0,
        os_type,
        engine_type: if device_count > 0 { 
            EngineType::Llama 
        } else { 
            EngineType::None 
        },
        usage: gpu_usage,        // GPU usage (estimated)
        mem_usage: gpu_mem_usage, // GPU memory usage (estimated)
        power_usage,             // GPU power usage (estimated)
        temp: temp as u64,
        vendor_id: first_vendor_id,
        device_id: first_device_id,
        memsize_gb: total_memory_gb as u128,
        powerlimit_w: if device_count > 0 { 300 } else { 0 } as u128, // Assume 300W power limit for GPUs
    };
    
    unsafe { instance.destroy_instance(None) };
    
    debug_println!("\n✅ Device information collection completed!");
    Ok((devices_info, device_count))
}

#[cfg(feature = "vulkan")]
fn estimate_gpu_tflops_cross_platform(vendor_id: u32, _device_id: u32, device_name: &[u8]) -> u16 {
    let binding = String::from_utf8_lossy(device_name);
    let name_str = binding.trim_end_matches('\0');
    
    // Basic TFLOPS estimation based on common GPUs (mobile and desktop)
    match vendor_id {
        // ARM Mali GPUs
        0x13B5 => {
            if name_str.contains("G715") || name_str.contains("G710") { 5 }
            else if name_str.contains("G68") || name_str.contains("G57") { 3 }
            else if name_str.contains("G52") || name_str.contains("G31") { 1 }
            else { 2 }
        },
        // Qualcomm Adreno GPUs
        0x5143 => {
            if name_str.contains("740") || name_str.contains("730") { 6 }
            else if name_str.contains("660") || name_str.contains("650") { 4 }
            else if name_str.contains("640") || name_str.contains("630") { 2 }
            else { 3 }
        },
        // PowerVR GPUs
        0x1010 => {
            if name_str.contains("BXS") { 4 }
            else if name_str.contains("XE") { 2 }
            else { 1 }
        },
        // NVIDIA GPUs
        0x10DE => {
            if name_str.contains("RTX 4090") { 83 }
            else if name_str.contains("RTX 4080") { 48 }
            else if name_str.contains("RTX 4070") { 29 }
            else if name_str.contains("RTX 4060") { 16 }
            else if name_str.contains("RTX 3090") { 36 }
            else if name_str.contains("RTX 3080") { 30 }
            else if name_str.contains("RTX 3070") { 20 }
            else if name_str.contains("RTX 3060") { 13 }
            else if name_str.contains("Tegra") { 8 }
            else { 10 }
        },
        // AMD GPUs
        0x1002 => {
            if name_str.contains("RX 7900") { 52 }
            else if name_str.contains("RX 7800") { 28 }
            else if name_str.contains("RX 7700") { 20 }
            else if name_str.contains("RX 6900") { 23 }
            else if name_str.contains("RX 6800") { 16 }
            else if name_str.contains("RX 6700") { 11 }
            else { 8 }
        },
        // Intel GPUs
        0x8086 => {
            if name_str.contains("Arc A770") { 16 }
            else if name_str.contains("Arc A750") { 12 }
            else if name_str.contains("Arc A380") { 8 }
            else if name_str.contains("Iris Xe") { 2 }
            else { 1 }
        },
        // Default estimation
        _ => 2,
    }
}

/// Try to get accurate GPU metrics from platform-specific APIs
/// Falls back to estimation if no accurate method is available
#[allow(dead_code)] // This function is used but may appear unused due to conditional compilation
fn get_accurate_gpu_metrics(
    vendor_id: u128, 
    device_count: u16, 
    total_tflops: u16, 
    total_memory_gb: u32
) -> (u64, u64, u64, u64) {
    
    // Try NVML first (most accurate for NVIDIA GPUs)
    #[cfg(all(feature = "nvml", not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Ok((usage, mem_usage, power, temp)) = try_nvml_metrics() {
            debug_println!("🎯 Using NVML for accurate GPU metrics");
            return (usage, mem_usage, power, temp);
        }
    }
    
    // Try ROCm SMI for AMD GPUs
    #[cfg(all(feature = "rocm", target_os = "linux"))]
    {
        if let Ok((usage, mem_usage, power, temp)) = try_rocm_metrics() {
            debug_println!("🎯 Using ROCm SMI for accurate GPU metrics");
            return (usage, mem_usage, power, temp);
        }
    }
    
    // Try WMI on Windows (if NVML is not available)
    #[cfg(all(target_os = "windows", not(feature = "nvml")))]
    {
        if let Ok((usage, mem_usage, power, temp)) = try_wmi_gpu_metrics() {
            debug_println!("🎯 Using WMI for accurate GPU metrics");
            return (usage, mem_usage, power, temp);
        }
    }
    
    // Try PowerMetrics on macOS
    #[cfg(target_os = "macos")]
    {
        if let Ok((usage, mem_usage, power, temp)) = try_macos_gpu_metrics() {
            debug_println!("🎯 Using PowerMetrics for accurate GPU metrics");
            return (usage, mem_usage, power, temp);
        }
    }
    
    // Try sysfs on Linux
    #[cfg(all(target_os = "linux", not(feature = "nvml")))]
    {
        if let Ok((usage, mem_usage, power, temp)) = try_sysfs_gpu_metrics(vendor_id) {
            debug_println!("🎯 Using sysfs for accurate GPU metrics");
            return (usage, mem_usage, power, temp);
        }
    }
    
    // Fallback to estimation (always available)
    debug_println!("⚠️  Using estimated GPU metrics (no accurate method available)");
    estimate_gpu_metrics_fallback(device_count, total_tflops, total_memory_gb)
}

/// Try to get GPU metrics using NVML (NVIDIA GPUs only)
#[cfg(all(feature = "nvml", not(target_os = "macos"), not(target_os = "android")))]
fn try_nvml_metrics() -> Result<(u64, u64, u64, u64), Box<dyn std::error::Error>> {
    use nvml_wrapper::NVML;
    
    let nvml = NVML::init()?;
    if let Ok(device) = nvml.device_by_index(0) {
        let utilization = device.utilization_rates()?;
        let memory_info = device.memory_info()?;
        let power_usage = device.power_usage()?;
        let temp = device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)?;
        
        let mem_usage_percent = if memory_info.total > 0 {
            (memory_info.used as f32 / memory_info.total as f32 * 100.0) as u64
        } else {
            0
        };
        
        return Ok((
            utilization.gpu as u64,
            mem_usage_percent,
            (power_usage / 1000) as u64, // Convert mW to W
            temp as u64
        ));
    }
    
    Err("No NVIDIA device found".into())
}

/// Try to get GPU metrics using ROCm SMI (AMD GPUs on Linux)
#[cfg(all(feature = "rocm", target_os = "linux"))]
fn try_rocm_metrics() -> Result<(u64, u64, u64, u64), Box<dyn std::error::Error>> {
    use rocm_smi_lib::*;
    
    // Initialize ROCm SMI
    let rocm = RocmSmi::new()?;
    
    // Get first AMD GPU
    if let Some(device) = rocm.devices().first() {
        // Get GPU utilization
        let utilization = device.get_utilization_rate()?;
        
        // Get memory information
        let memory_info = device.get_memory_info()?;
        let mem_usage_percent = if memory_info.total > 0 {
            (memory_info.used as f32 / memory_info.total as f32 * 100.0) as u64
        } else {
            0
        };
        
        // Get power usage (in watts)
        let power_usage = device.get_power_usage()? / 1000000; // Convert microwatts to watts
        
        // Get temperature
        let temp = device.get_temperature(TemperatureSensor::Gpu)?;
        
        return Ok((
            utilization as u64,
            mem_usage_percent,
            power_usage,
            temp as u64
        ));
    }
    
    Err("No AMD GPU with ROCm support found".into())
}

/// Try to get GPU metrics using WMI (Windows)
#[cfg(all(target_os = "windows", not(feature = "nvml")))]
fn try_wmi_gpu_metrics() -> Result<(u64, u64, u64, u64), Box<dyn std::error::Error>> {
    // WMI on most consumer systems doesn't provide real-time GPU metrics
    // Performance counters are often unavailable or return errors
    debug_println!("⚠️  WMI GPU metrics not available on this system");
    debug_println!("💡 Note: Consider installing NVML for NVIDIA GPU monitoring");
    
    // Use simple fallback instead of complex estimation
    Err("WMI GPU metrics not available".into())
}

/// Fallback GPU metrics estimation using system CPU load and heuristic coefficients
#[allow(dead_code)] // This function is used but may appear unused due to conditional compilation
fn estimate_gpu_metrics_fallback(_device_count: u16, _total_tflops: u16, _total_memory_gb: u32) -> (u64, u64, u64, u64) {
    // When no accurate method is available, return conservative default values
    // This is better than complex estimation that might be misleading
    (0, 0, 8, 35) // Usage: 0%, Memory: 0%, Power: 8W (idle), Temp: 35°C (ambient)
}

/// Try to get GPU metrics using sysfs (Linux)
#[cfg(all(target_os = "linux", not(feature = "nvml")))]
fn try_sysfs_gpu_metrics(_vendor_id: u128) -> Result<(u64, u64, u64, u64), Box<dyn std::error::Error>> {
    // TODO: Implement actual sysfs reading logic based on vendor_id
    // For AMD: /sys/class/drm/card0/device/gpu_busy_percent
    // For Intel: /sys/class/drm/card0/gt/gt0/freq_mhz (approximation)
    // For now, best-effort implementation using common sysfs/hwmon nodes.

    fn read_trimmed(path: &std::path::Path) -> std::io::Result<String> {
        Ok(std::fs::read_to_string(path)?.trim().to_string())
    }

    fn parse_u64(s: &str) -> Option<u64> {
        s.trim().parse::<u64>().ok()
    }

    fn parse_hex_u32(s: &str) -> Option<u32> {
        let s = s.trim();
        let s = s.strip_prefix("0x").unwrap_or(s);
        u32::from_str_radix(s, 16).ok()
    }

    let drm_dir = std::path::Path::new("/sys/class/drm");
    let entries = std::fs::read_dir(drm_dir)?;

    let mut candidates: Vec<(std::path::PathBuf, Option<u32>)> = Vec::new();
    for e in entries {
        let e = e?;
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") {
            continue;
        }
        if name.contains("-") {
            continue;
        }

        let device_dir = e.path().join("device");
        if !device_dir.exists() {
            continue;
        }

        let vendor = read_trimmed(&device_dir.join("vendor")).ok().and_then(|v| parse_hex_u32(&v));
        candidates.push((device_dir, vendor));
    }

    if candidates.is_empty() {
        return Err("No DRM devices found in /sys/class/drm".into());
    }

    let wanted_vendor = if _vendor_id > u32::MAX as u128 {
        None
    } else {
        Some(_vendor_id as u32)
    };

    let device_dir = candidates
        .iter()
        .find(|(_, v)| wanted_vendor.is_some() && v.is_some() && v.unwrap() == wanted_vendor.unwrap())
        .map(|(p, _)| p.clone())
        .or_else(|| candidates.first().map(|(p, _)| p.clone()))
        .ok_or("No usable DRM device found")?;

    // Utilization
    let usage = {
        let busy_paths = [
            device_dir.join("gpu_busy_percent"),
            device_dir.join("gt_busy_percent"),
            device_dir.join("busy_percent"),
        ];
        let mut val: Option<u64> = None;
        for p in &busy_paths {
            if let Ok(s) = read_trimmed(p) {
                val = parse_u64(&s);
                if val.is_some() {
                    break;
                }
            }
        }
        val.unwrap_or(0).min(100)
    };

    // VRAM usage (percentage)
    let mem_usage = {
        let total = read_trimmed(&device_dir.join("mem_info_vram_total")).ok().and_then(|s| parse_u64(&s));
        let used = read_trimmed(&device_dir.join("mem_info_vram_used")).ok().and_then(|s| parse_u64(&s));
        match (total, used) {
            (Some(t), Some(u)) if t > 0 => ((u as f64 / t as f64) * 100.0).round() as u64,
            _ => 0,
        }
    };

    // Power (W) and temperature (°C) from hwmon if available
    let (power_usage, temp) = {
        let mut power_w: u64 = 0;
        let mut temp_c: u64 = 0;

        let hwmon_base = device_dir.join("hwmon");
        if let Ok(hwmons) = std::fs::read_dir(&hwmon_base) {
            for h in hwmons.flatten() {
                let hpath = h.path();
                let power_paths = [
                    hpath.join("power1_average"),
                    hpath.join("power1_input"),
                ];
                for p in &power_paths {
                    if let Ok(s) = read_trimmed(p) {
                        if let Some(uw) = parse_u64(&s) {
                            power_w = (uw / 1_000_000).max(0);
                            break;
                        }
                    }
                }

                let temp_paths = [hpath.join("temp1_input"), hpath.join("temp2_input")];
                for t in &temp_paths {
                    if let Ok(s) = read_trimmed(t) {
                        if let Some(millic) = parse_u64(&s) {
                            temp_c = (millic / 1000).max(0);
                            break;
                        }
                    }
                }

                if power_w > 0 || temp_c > 0 {
                    break;
                }
            }
        }

        (power_w, temp_c)
    };

    if usage == 0 && mem_usage == 0 && power_usage == 0 && temp == 0 {
        return Err("Sysfs GPU metrics not available on this system".into());
    }

    Ok((usage, mem_usage.min(100), power_usage, temp))
}
