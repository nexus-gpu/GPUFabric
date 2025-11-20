use anyhow::{Result, anyhow};
use clap::Parser;
use gpuf_c::{init, create_worker, config::Args, WorkerHandle};

#[cfg(not(target_os = "android"))]
use gpuf_c::llm_engine::{llama_engine::LlamaEngine, llama_server::start_server, Engine};
#[cfg(not(target_os = "android"))]
use std::sync::Arc;
#[cfg(not(target_os = "android"))]
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<()> {
    init()?;

    let args = Args::parse().load_config()?;

    // Check if running in standalone LLAMA mode
    #[cfg(not(target_os = "android"))]
    if args.standalone_llama {
        return run_standalone_llama(args).await;
    }

    // Normal GPUFabric worker mode
    let worker = create_worker(args).await?;

    worker.login().await?;
    worker.handler().await?;
    Ok(())
}

#[cfg(not(target_os = "android"))]
async fn run_standalone_llama(mut args: Args) -> Result<()> {
    use tracing::info;
    
    info!("Starting standalone LLAMA API server mode");
    
    // If using default Worker port (11434), change to standalone default (8080)
    if args.local_port == 11434 {
        args.local_port = 8080;
        info!("Using standalone default port 8080");
    }
    
    // Get model path
    let model_path = if let Some(path) = args.llama_model_path {
        path
    } else {
        // Auto-download default model
        info!("No model specified, downloading default TinyLlama model...");
        let models_dir = dirs::home_dir()
            .ok_or_else(|| anyhow!("Could not determine home directory"))?
            .join(".llama")
            .join("models");
        
        std::fs::create_dir_all(&models_dir)?;
        
        let model_name = "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf";
        let model_path = models_dir.join(model_name);
        
        if !model_path.exists() {
            info!("Downloading {} (~600MB)...", model_name);
            let url = "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf";
            
            let response = reqwest::get(url).await?;
            let bytes = response.bytes().await?;
            std::fs::write(&model_path, bytes)?;
            
            info!("Model downloaded successfully!");
        } else {
            info!("Using existing model at {:?}", model_path);
        }
        
        model_path.to_string_lossy().to_string()
    };
    
    info!("Loading model: {}", model_path);
    
    // Create and initialize engine
    let mut engine = LlamaEngine::with_config(
        model_path.clone(),
        2048,  // context size
        35,    // GPU layers (adjust based on your GPU)
    );
    
    engine.init().await?;
    engine.start_worker().await?;
    
    info!("Model loaded successfully!");
    info!("Engine ready: {}", engine.is_ready().await);
    
    // Start API server
    let host = args.local_addr.clone();
    let port = args.local_port;
    
    info!("Starting API server on {}:{}", host, port);
    info!("OpenAI compatible endpoints:");
    info!("  - POST http://{}:{}/v1/chat/completions", host, port);
    info!("  - POST http://{}:{}/v1/completions", host, port);
    info!("  - GET  http://{}:{}/v1/models", host, port);
    info!("  - GET  http://{}:{}/health", host, port);
    
    let engine = Arc::new(RwLock::new(engine));
    start_server(engine, &host, port).await?;
    
    Ok(())
}
