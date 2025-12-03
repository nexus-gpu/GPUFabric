//! Performance test example
//! 
//! Compare performance differences between different forwarding schemes

use anyhow::Result;
use std::time::Instant;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting performance comparison test");

    // Test parameters
    let num_requests = 50;
    let prompt = "Rust is a systems programming language that emphasizes safety, speed, and concurrency. It is designed to prevent common programming errors such as null pointer dereferences, data races, and buffer overflows through its ownership and borrowing system.";

    // 1. Test HTTP forwarding performance
    println!("📡 Testing HTTP forwarding performance...");
    let http_time = test_http_forwarding(prompt, num_requests).await?;
    
    // 2. Test Unix Socket performance
    println!("🔌 Testing Unix Socket performance...");
    let socket_time = test_unix_socket(prompt, num_requests).await?;
    
    // 3. Test shared memory performance
    println!("🧠 Testing shared memory performance...");
    let shared_time = test_shared_memory(prompt, num_requests).await?;

    // Output comparison results
    println!("\n=== Performance Comparison Results ===");
    println!("📡 HTTP Forwarding:  {:.2} ms avg, {:.1} req/s", http_time, 1000.0 / http_time);
    println!("🔌 Unix Socket:      {:.2} ms avg, {:.1} req/s", socket_time, 1000.0 / socket_time);
    println!("🧠 Shared Memory:    {:.2} ms avg, {:.1} req/s", shared_time, 1000.0 / shared_time);
    
    println!("\n📊 Overhead Analysis:");
    println!("📡 HTTP overhead:     +{:.1}ms vs shared memory", http_time - shared_time);
    println!("🔌 Socket overhead:   +{:.1}ms vs shared memory", socket_time - shared_time);

    Ok(())
}

async fn test_http_forwarding(prompt: &str, num_requests: usize) -> Result<f64> {
    let start = Instant::now();
    
    for i in 0..num_requests {
        let _test_prompt = format!("{} #{}", prompt, i);
        
        // Simulate HTTP request delay (1-2ms)
        sleep(Duration::from_millis(1 + (i % 2) as u64)).await;
        
        // Simulate inference delay (50-100ms)
        sleep(Duration::from_millis(50 + (i % 51) as u64)).await;
    }
    
    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() as f64 / num_requests as f64;
    
    Ok(avg_ms)
}

async fn test_unix_socket(prompt: &str, num_requests: usize) -> Result<f64> {
    let start = Instant::now();
    
    for i in 0..num_requests {
        let _test_prompt = format!("{} #{}", prompt, i);
        
        // Simulate Unix Socket delay (0.3-0.5ms)
        sleep(Duration::from_micros(300 + (i % 200) as u64 * 1000)).await;
        
        // Simulate inference delay (50-100ms)
        sleep(Duration::from_millis(50 + (i % 51) as u64)).await;
    }
    
    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() as f64 / num_requests as f64;
    
    Ok(avg_ms)
}

async fn test_shared_memory(prompt: &str, num_requests: usize) -> Result<f64> {
    let start = Instant::now();
    
    for i in 0..num_requests {
        let _test_prompt = format!("{} #{}", prompt, i);
        
        // Simulate shared memory delay (0.1-0.2ms)
        sleep(Duration::from_micros(100 + (i % 100) as u64 * 1000)).await;
        
        // Simulate inference delay (50-100ms)
        sleep(Duration::from_millis(50 + (i % 51) as u64)).await;
    }
    
    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() as f64 / num_requests as f64;
    
    Ok(avg_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_performance_comparison() {
        let result = main().await;
        assert!(result.is_ok());
    }
}
