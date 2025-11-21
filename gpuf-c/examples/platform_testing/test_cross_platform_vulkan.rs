//! Cross-platform Vulkan device information collection test example
//! 
//! This example directly tests the new cross-platform Vulkan module

#[cfg(feature = "vulkan")]
use anyhow::Result;
#[cfg(feature = "vulkan")]
use gpuf_c::util::system_info_vulkan::collect_device_info_vulkan_cross_platform;

#[cfg(feature = "vulkan")]
#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting cross-platform Vulkan device info collection test...");
    
    // Test device information collection
    match collect_device_info_vulkan_cross_platform().await {
        Ok((device_info, device_count)) => {
            println!("\n🎉 Device information collection successful!");
            println!("📊 Returned structured information:");
            println!("  Device Count: {}", device_info.num);
            println!("  Total TFLOPS: {}", device_info.total_tflops);
            println!("  System Memory: {}GB", device_info.memtotal_gb);
            println!("  {}GB", device_info.memsize_gb);
            println!("  Vendor ID: 0x{:04x}", device_info.vendor_id);
            println!("  Device ID: 0x{:04x}", device_info.device_id);
            println!("  Operating System Type: {:?}", device_info.os_type);
            println!("  Engine Type: {:?}", device_info.engine_type);
            println!("  CPU Usage: {}%", device_info.usage);
            println!("  Memory Usage: {}%", device_info.mem_usage);
            println!("  Estimated Power: {}W", device_info.power_usage);
            println!("  Estimated Temperature: {}°C", device_info.temp);
            println!("  Power Limit: {}W", device_info.powerlimit_w);
            
            // Verify data integrity
            println!("\n🔍 Data integrity check:");
            if device_count > 0 {
                println!("  ✅ GPU device detected");
                if device_info.total_tflops > 0 {
                    println!("  ✅ TFLOPS calculation correct");
                } else {
                    println!("  ⚠️  TFLOPS is 0, estimation may need optimization");
                }
                if device_info.memsize_gb > 0 {
                    println!("  ✅ GPU memory detection correct");
                } else {
                    println!("  ⚠️  GPU memory is 0, Vulkan detection may have failed");
                }
                if device_info.power_usage > 0 {
                    println!("  ✅ Power estimation correct");
                } else {
                    println!("  ⚠️  Power is 0, estimation may need optimization");
                }
                if device_info.temp > 0 {
                    println!("  ✅ Temperature estimation correct");
                } else {
                    println!("  ⚠️  Temperature is 0, estimation may need optimization");
                }
            } else {
                println!("  ⚠️  No GPU device detected");
            }
            
            if device_info.memtotal_gb > 0 {
                println!("  ✅ System memory detection correct");
            } else {
                println!("  ❌ System memory detection failed");
            }
            
            if device_info.usage > 0 || device_info.mem_usage > 0 {
                println!("  ✅ System monitoring data correct");
            } else {
                println!("  ⚠️  System monitoring data is empty");
            }
            
        }
        Err(e) => {
            println!("❌ Device information collection failed: {}", e);
            
            // Provide troubleshooting suggestions
            println!("\n🛠️  Troubleshooting suggestions:");
            if e.to_string().contains("Vulkan") {
                println!("  • Ensure device supports Vulkan");
                println!("  • Try updating graphics drivers");
                println!("  • Check if Vulkan runtime is installed");
            } else {
                println!("  • Check application permissions");
                println!("  • Ensure hardware access permissions");
            }
        }
    }
    
    Ok(())
}

#[cfg(not(feature = "vulkan"))]
fn main() {
    println!("❌ This example requires the 'vulkan' feature to be enabled.");
    println!("Please run with: cargo run --example test_cross_platform_vulkan --features vulkan");
}
