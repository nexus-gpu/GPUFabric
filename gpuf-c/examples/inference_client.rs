//! Inference service client example
//!
//! Show how to communicate with standalone inference service

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct InferenceRequest {
    prompt: String,
    max_tokens: Option<usize>,
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct InferenceResponse {
    text: String,
    tokens_used: usize,
    generation_time_ms: u64,
    #[allow(dead_code)] // Future expansion use, currently not read
    finished: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting inference client example");

    let client = Client::new();
    let service_url = "http://127.0.0.1:8082";

    // 1. Check service health status
    println!("🔍 Checking service health...");
    let health_response = client
        .get(&format!("{}/health", service_url))
        .send()
        .await?;

    if health_response.status().is_success() {
        let health: serde_json::Value = health_response.json().await?;
        println!("✅ Service health: {:?}", health);
    } else {
        eprintln!("❌ Service is not healthy: {}", health_response.status());
        return Ok(());
    }

    // 2. Send inference request
    println!("📤 Sending inference request...");
    let request = InferenceRequest {
        prompt: "Rust is a programming language that".to_string(),
        max_tokens: Some(100),
        temperature: Some(0.7),
    };

    let response = client
        .post(&format!("{}/v1/completions", service_url))
        .json(&request)
        .send()
        .await?;

    if response.status().is_success() {
        let result: InferenceResponse = response.json().await?;
        println!("📝 Generated text: {}", result.text);
        println!("🔢 Tokens used: {}", result.tokens_used);
        println!("⏱️  Generation time: {}ms", result.generation_time_ms);
    } else {
        eprintln!("❌ Inference request failed: {}", response.status());
        let error_text = response.text().await?;
        eprintln!("🔍 Error details: {}", error_text);
    }

    // 3. Get service statistics
    println!("📊 Getting service statistics...");
    let stats_response = client.get(&format!("{}/stats", service_url)).send().await?;

    if stats_response.status().is_success() {
        let stats: serde_json::Value = stats_response.json().await?;
        println!("📈 Service stats: {:?}", stats);
    }

    println!("✅ Client example completed");
    Ok(())
}
