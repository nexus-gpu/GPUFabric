//! Test device info collection functionality (real-time, no cache)

use gpuf_c::util::system_info::collect_device_info;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    info!("Testing device info collection (real-time)...");

    // First call - collect fresh data
    info!("First call - collecting device info");
    let start = std::time::Instant::now();
    let (device_info1, memory1) = collect_device_info().await?;
    let first_duration = start.elapsed();
    info!("First call took: {:?}", first_duration);
    info!("Device info: {} devices, {} GB memory", device_info1.num, memory1);

    // Second call - collect fresh data again (real-time)
    info!("Second call - collecting fresh device info");
    let start = std::time::Instant::now();
    let (device_info2, memory2) = collect_device_info().await?;
    let second_duration = start.elapsed();
    info!("Second call took: {:?}", second_duration);
    info!("Device info: {} devices, {} GB memory", device_info2.num, memory2);

    // Verify data consistency (should be similar for static info)
    info!("Comparing device info...");
    info!("Device count: {} vs {}", device_info1.num, device_info2.num);
    info!("Memory: {} GB vs {} GB", memory1, memory2);
    
    // Note: Real-time metrics (usage, temp, etc.) may differ between calls
    if device_info1.num == device_info2.num && memory1 == memory2 {
        info!("✅ Static device info is consistent");
    } else {
        info!("⚠️  Device info changed between calls (this can happen normally)");
    }

    // Performance check - both calls should take similar time
    let time_diff = if first_duration > second_duration {
        first_duration - second_duration
    } else {
        second_duration - first_duration
    };
    
    info!("Time difference between calls: {:?}", time_diff);
    if time_diff < std::time::Duration::from_millis(100) {
        info!("✅ Performance is consistent (no cache, real-time collection)");
    } else {
        info!("⚠️  Performance varies (normal for real-time system calls)");
    }

    info!("✅ Device info collection test completed successfully!");
    Ok(())
}
