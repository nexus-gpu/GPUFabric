// Example: Llama.cpp API Server
// Start an OpenAI compatible HTTP API server using embedded LlamaEngine

use gpuf_c::llm_engine::llama_engine::LlamaEngine;
use gpuf_c::llm_engine::llama_server::start_server;
use gpuf_c::llm_engine::Engine;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, Level};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    // Get configuration from command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    let host = args.get(2).map(|s| s.as_str()).unwrap_or("127.0.0.1");
    let port: u16 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8080);

    // Get or download model
    let model_path = if let Some(path) = args.get(1) {
        path.clone()
    } else {
        info!("No model specified, will download default model (TinyLlama-1.1B-Chat)");
        let engine = LlamaEngine::new();
        
        // Check if model already exists
        let models = engine.list_local_models().await?;
        if let Some(existing_model) = models.first() {
            info!("Found existing model: {}", existing_model);
            engine.models_dir.join(existing_model).to_string_lossy().to_string()
        } else {
            info!("Downloading TinyLlama-1.1B-Chat-v1.0 (~600MB)...");
            info!("This may take a few minutes depending on your network speed.");
            
            // TinyLlama 1.1B Q4_K_M - Very small model, suitable for testing
            let url = "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf";
            let filename = "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf";
            
            match engine.download_model(url, filename).await {
                Ok(path) => {
                    info!("Model downloaded successfully!");
                    path.to_string_lossy().to_string()
                }
                Err(e) => {
                    eprintln!("Failed to download model: {}", e);
                    eprintln!("Please download a GGUF model manually and specify the path:");
                    eprintln!("  llama_api_server <model_path> [host] [port]");
                    std::process::exit(1);
                }
            }
        }
    };

    info!("Initializing Llama.cpp engine with model: {}", model_path);

    // Create and initialize engine
    let mut engine = LlamaEngine::with_config(
        model_path.clone(),
        2048,  // context size
        35,    // GPU layers (adjust based on your GPU memory)
    );

    info!("Loading model...");
    engine.init().await?;
    engine.start_worker().await?;

    info!("Model loaded successfully!");
    info!("Engine ready: {}", engine.is_ready().await);

    // Wrap as Arc<RwLock<>>
    let engine = Arc::new(RwLock::new(engine));

    // Start HTTP server
    info!("Starting API server on {}:{}", host, port);
    info!("OpenAI compatible endpoints:");
    info!("  - POST http://{}:{}/v1/chat/completions", host, port);
    info!("  - POST http://{}:{}/v1/completions", host, port);
    info!("  - GET  http://{}:{}/v1/models", host, port);
    info!("  - GET  http://{}:{}/health", host, port);

    start_server(engine, host, port).await?;

    Ok(())
}
