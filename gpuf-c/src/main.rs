use anyhow::{anyhow, Result};
use clap::Parser;
use gpuf_c::{
    handle::{new_worker, WorkerHandle},
    util::cmd::Args,
    util::init_logging,
};

#[cfg(not(target_os = "android"))]
use gpuf_c::llm_engine::{
    llama_engine::LlamaEngine,
    llama_server::{start_server_with_security, ServerSecurityConfig},
    Engine,
};
#[cfg(not(target_os = "android"))]
use std::sync::Arc;
#[cfg(not(target_os = "android"))]
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    std::panic::set_hook(Box::new(|info| {
        eprintln!("gpuf-c panic: {info}");
    }));

    let args = Args::parse().load_config()?;

    // Check if running in standalone LLAMA mode
    #[cfg(not(target_os = "android"))]
    if args.standalone_llama {
        return run_standalone_llama(args).await;
    }

    // Normal GPUFabric worker mode
    loop {
        let worker = new_worker(args.clone()).await;

        if let Err(e) = worker.login().await {
            tracing::error!(error = %e, "gpuf-c login failed");
            drop(worker); // Explicitly drop worker to free resources
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        if let Err(e) = worker.handler().await {
            tracing::error!(error = %e, "gpuf-c handler exited");
            drop(worker); // Explicitly drop worker to free resources
            tracing::info!("Waiting for resources to be freed before reconnecting...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
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
    let requested = args
        .llama_model_path
        .clone()
        .ok_or_else(|| anyhow!("Model path not set"))?;

    let models_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join(".llama")
        .join("models");
    std::fs::create_dir_all(&models_dir)?;

    let model_path_buf = {
        let p = std::path::Path::new(&requested);
        if p.components().count() == 1 {
            models_dir.join(p)
        } else {
            p.to_path_buf()
        }
    };

    if !model_path_buf.exists() {
        return Err(anyhow!(
            "Model file not found: {}. Download models explicitly with a verified SHA256 checksum before starting standalone mode.",
            model_path_buf.to_string_lossy()
        ));
    } else {
        info!("Using existing model at {:?}", model_path_buf);
    }

    let model_path = model_path_buf.to_string_lossy().to_string();

    info!("Loading model: {}", model_path);
    info!("Configuration:");
    info!("  - Context size: {}", args.n_ctx);
    info!("  - Batch size: {}", args.n_batch);
    info!("  - GPU layers: {}", args.n_gpu_layers);

    // Create and initialize engine
    let mut engine = LlamaEngine::with_config(
        model_path.clone(),
        args.n_ctx,        // context size from args
        args.n_batch,      // batch size from args
        args.n_gpu_layers, // GPU layers from args
        args.llama_split_mode.clone(),
        args.llama_main_gpu,
        args.llama_devices.clone(),
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
    let mut security = ServerSecurityConfig::from_env();
    if let Some(api_key) = args
        .api_key
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        security.api_key = Some(api_key);
    }
    start_server_with_security(engine, &host, port, security).await?;

    Ok(())
}
