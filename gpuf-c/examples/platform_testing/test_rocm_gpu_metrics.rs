//! Test ROCm GPU metrics collection for AMD GPUs

#[cfg(all(feature = "rocm", target_os = "linux"))]
use anyhow::Result;
#[cfg(all(feature = "rocm", target_os = "linux"))]
use gpuf_c::util::system_info_vulkan::collect_device_info_vulkan_cross_platform;
#[cfg(all(feature = "rocm", target_os = "linux"))]
use tracing::{info, Level};
#[cfg(all(feature = "rocm", target_os = "linux"))]
use tracing_subscriber;

#[cfg(all(feature = "rocm", target_os = "linux"))]
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    info!("🔍 Testing ROCm GPU metrics collection...");

    // Test the ROCm-enabled Vulkan device info collection
    let (device_info, device_count) = collect_device_info_vulkan_cross_platform().await?;
    
    info!("✅ Successfully collected device information!");
    info!("📊 Device Count: {}", device_count);
    info!("🎮 GPU Usage: {}%", device_info.usage);
    info!("💾 GPU Memory Usage: {}%", device_info.mem_usage);
    info!("🔌 GPU Power: {}W", device_info.power_usage);
    info!("🌡️  GPU Temperature: {}°C", device_info.temp);
    info!("⚡ Total TFLOPS: {}", device_info.total_tflops);
    info!("💰 Total Memory: {}GB", device_info.memtotal_gb);

    // Validate metrics are in reasonable ranges
    assert!(device_info.usage <= 100, "GPU usage should not exceed 100%");
    assert!(device_info.mem_usage <= 100, "GPU memory usage should not exceed 100%");
    assert!(device_info.power_usage <= 1000, "GPU power usage should not exceed 1000W");
    assert!(device_info.temp <= 120, "GPU temperature should not exceed 120°C");
    
    info!("✅ All GPU metrics are within reasonable ranges!");
    info!("🎯 ROCm GPU metrics test completed successfully!");
    
    Ok(())
}

#[cfg(not(all(feature = "rocm", target_os = "linux")))]
fn main() {
    println!("❌ This example requires the 'rocm' feature and Linux OS to run.");
    println!("Please run with: cargo run --example test_rocm_gpu_metrics --features 'vulkan,rocm'");
    println!("Note: ROCm is only available on Linux with AMD GPUs and ROCm drivers installed.");
}
