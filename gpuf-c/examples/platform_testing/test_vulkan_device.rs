//! Vulkan device detection and test example
//! 
//! This example demonstrates how to use Vulkan API to detect GPU information on Android devices

#[cfg(feature = "vulkan")]
use anyhow::Result;
#[cfg(feature = "vulkan")]
use tracing::{info, error};

#[cfg(feature = "vulkan")]
fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    println!("Starting Vulkan device detection...");
    info!("Starting Vulkan device detection...");
    
    // Test Vulkan device detection
    match test_vulkan_devices() {
        Ok(device_count) => {
            println!("Vulkan device detection completed, found {} devices", device_count);
            info!("Vulkan device detection completed, found {} devices", device_count);
        }
        Err(e) => {
            println!("Vulkan device detection failed: {}", e);
            error!("Vulkan device detection failed: {}", e);
        }
    }
    
    Ok(())
}

#[cfg(not(feature = "vulkan"))]
fn main() {
    println!("This example requires the 'vulkan' feature to be enabled.");
    println!("Please run with: cargo run --example test_vulkan_device --features vulkan");
}

#[cfg(feature = "vulkan")]
fn test_vulkan_devices() -> Result<usize> {
    use ash::{vk, Entry};
    
    // 1. Load Vulkan entry point
    let entry = unsafe { Entry::load() }
        .map_err(|e| anyhow::anyhow!("Failed to load Vulkan entry point: {}", e))?;
    
    info!("✓ Vulkan entry point loaded successfully");
    
    // 2. Create Vulkan instance
    let app_info = vk::ApplicationInfo::builder()
        .application_name(c"GPUFabric Vulkan Test")
        .application_version(vk::make_api_version(0, 1, 0, 0))
        .engine_name(c"GPUFabric")
        .engine_version(vk::make_api_version(0, 1, 0, 0))
        .api_version(vk::API_VERSION_1_0);
    
    let create_info = vk::InstanceCreateInfo::builder()
        .application_info(&app_info);
    
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(|e| anyhow::anyhow!("Failed to create Vulkan instance: {}", e))?;
    
    info!("✓ Vulkan instance created successfully");
    
    // 3. Enumerate physical devices
    let physical_devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| anyhow::anyhow!("Failed to enumerate physical devices: {}", e))?;
    
    info!("✓ Found {} Vulkan physical devices", physical_devices.len());
    
    // 4. Get detailed information for each device
    for (index, &physical_device) in physical_devices.iter().enumerate() {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let features = unsafe { instance.get_physical_device_features(physical_device) };
        let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical_device) };
        
        let device_name = unsafe {
        std::ffi::CStr::from_ptr(properties.device_name.as_ptr())
            .to_string_lossy()
    };
        
        info!("\n=== Device {} ===", index + 1);
        info!("Device Name: {}", device_name);
        info!("Device Type: {:?}", properties.device_type);
        info!("Vendor ID: 0x{:04x}", properties.vendor_id);
        info!("Device ID: 0x{:04x}", properties.device_id);
        info!("API Version: {}.{}.{}", 
            vk::version_major(properties.api_version),
            vk::version_minor(properties.api_version),
            vk::version_patch(properties.api_version)
        );
        
        // Calculate device memory
        let mut device_memory = 0u64;
        let mut heap_count = 0;
        for (i, heap) in memory_properties.memory_heaps.iter().enumerate() {
            info!("Memory heap {}: {}MB, flags: {:?}", i, heap.size / (1024 * 1024), heap.flags);
            if heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
                device_memory += heap.size;
                heap_count += 1;
            }
        }
        
        info!("Device dedicated memory: {}GB ({} heaps)", 
            device_memory / (1024 * 1024 * 1024), heap_count);
        
        // Calculate TFLOPS estimation
        let tflops = estimate_gpu_tflops(properties.vendor_id, properties.device_id, device_name.as_bytes());
        info!("Performance estimate: {} TFLOPS", tflops);
        
        // Check key features
        info!("Supported features:");
        info!("  Geometry Shader: {}", features.geometry_shader != 0);
        info!("  Tessellation Shader: {}", features.tessellation_shader != 0);
        info!("  Multi Viewport: {}", features.multi_viewport != 0);
        info!("  Shader Storage Buffer: {}", features.shader_storage_image_extended_formats != 0);
        
        // Get queue family information
        let queue_families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        info!("Queue family count: {}", queue_families.len());
        
        for (i, queue_family) in queue_families.iter().enumerate() {
            let mut capabilities = Vec::new();
            if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                capabilities.push("Graphics");
            }
            if queue_family.queue_flags.contains(vk::QueueFlags::COMPUTE) {
                capabilities.push("Compute");
            }
            if queue_family.queue_flags.contains(vk::QueueFlags::TRANSFER) {
                capabilities.push("Transfer");
            }
            if queue_family.queue_flags.contains(vk::QueueFlags::SPARSE_BINDING) {
                capabilities.push("Sparse Binding");
            }
            
            info!("  Queue family {}: {} queues, capabilities: {:?}", 
                i, queue_family.queue_count, capabilities.join(", "));
        }
    }
    
    // 5. Clean up resources
    unsafe { instance.destroy_instance(None) };
    
    Ok(physical_devices.len())
}

#[cfg(feature = "vulkan")]
fn estimate_gpu_tflops(vendor_id: u32, _device_id: u32, device_name: &[u8]) -> u32 {
    let binding = String::from_utf8_lossy(device_name);
    let name_str = binding.trim_end_matches('\0');
    
    // TFLOPS estimation based on common mobile GPUs
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
        // NVIDIA (rare in mobile)
        0x10DE => {
            if name_str.contains("Tegra") { 8 }
            else { 5 }
        },
        // Default estimation
        _ => 2,
    }
}
