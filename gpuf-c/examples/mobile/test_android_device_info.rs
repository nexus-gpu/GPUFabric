//! Android device information collection test example
//! 
//! This example demonstrates how to test the improved Android device information collection functionality

use anyhow::Result;
use gpuf_c::util::system_info::collect_device_info;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting Android device info collection test...");
    
    // Test device information collection
    match collect_device_info().await {
        Ok((device_info, device_count)) => {
            println!("\n🎉 Device information collection successful!");
            println!("📊 Returned structured information:");
            println!("  Device Count: {}", device_info.num);
            println!("  Total TFLOPS: {}", device_info.total_tflops);
            println!("  System Memory: {}GB", device_info.memtotal_gb);
            println!("  GPU Memory: {}GB", device_info.memsize_gb);
            println!("  Vendor ID: 0x{:04x}", device_info.vendor_id);
            println!("  Device ID: 0x{:04x}", device_info.device_id);
            println!("  Operating System Type: {:?}", device_info.os_type);
            println!("  Engine Type: {:?}", device_info.engine_type);
            
            // Validate data integrity
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
            } else {
                println!("  ⚠️  No GPU device detected");
            }
            
            if device_info.memtotal_gb > 0 {
                println!("  ✅ System memory detection correct");
            } else {
                println!("  ❌ System memory detection failed");
            }
            
        }
        Err(e) => {
            println!("❌ Device information collection failed: {}", e);
            
            // Provide troubleshooting suggestions
            println!("\n🛠️  Troubleshooting suggestions:");
            if e.to_string().contains("Vulkan") {
                println!("  • Ensure device supports Vulkan");
                println!("  • Try enabling vulkan feature: --features vulkan");
                println!("  • Check if Vulkan driver is installed");
            }
            if e.to_string().contains("permission") {
                println!("  • Check application permissions");
                println!("  • Ensure hardware access permissions");
            }
        }
    }
    
    Ok(())
}
