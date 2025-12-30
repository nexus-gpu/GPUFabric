//! Example usage of the model downloader with parallel downloading and resume support

use crate::llm_engine::Engine;
use crate::util::model_downloader::{DownloadConfig, DownloadProgress, ModelDownloader};
use anyhow::Result;
use std::path::PathBuf;

/// Download a Llama model with progress tracking and resume support
pub async fn download_llama_model() -> Result<()> {
    let model_url = "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf";

    let config = DownloadConfig {
        url: model_url.to_string(),
        output_path: dirs::home_dir()
            .unwrap_or_default()
            .join(".llama")
            .join("models")
            .join("tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"),
        parallel_chunks: 8,               // Download in 8 parallel chunks
        chunk_size: 16 * 1024 * 1024,     // 16MB chunks
        expected_size: Some(668_066_816), // Expected file size
        checksum: Some(
            "7e5a3a8a9c8f5b2d4e6a1b3c7f9e8d5a2b4c6d8e7f9a1b3c5d7e8f9a2b4c6d8".to_string(),
        ), // Example checksum
        resume: true,
    };

    let mut downloader = ModelDownloader::new(config);

    // Set progress callback
    downloader.set_progress_callback(|progress: DownloadProgress| {
        let percentage = progress.percentage * 100.0;
        let downloaded_mb = progress.downloaded_bytes / (1024 * 1024);
        let total_mb = progress.total_bytes / (1024 * 1024);

        println!(
            "Download: {:.1}% ({}/{} MB) - Speed: {:.1} MB/s",
            percentage,
            downloaded_mb,
            total_mb,
            progress.speed_bps / (1024 * 1024)
        );

        if let Some(eta) = progress.eta_seconds {
            println!("ETA: {} seconds", eta);
        }
    });

    downloader.download().await?;

    println!("✅ Model downloaded successfully!");
    Ok(())
}

/// Simple download without progress tracking
pub async fn simple_download() -> Result<()> {
    let url = "https://example.com/model.bin";
    let output_path = PathBuf::from("/path/to/model.bin");

    crate::util::model_downloader::download_model(url, &output_path).await?;

    println!("✅ Download completed!");
    Ok(())
}

/// Download with custom configuration for slow networks
pub async fn download_for_slow_network() -> Result<()> {
    let config = DownloadConfig {
        url: "https://example.com/large-model.gguf".to_string(),
        output_path: PathBuf::from("/path/to/large-model.gguf"),
        parallel_chunks: 2,          // Fewer chunks for slow networks
        chunk_size: 4 * 1024 * 1024, // Smaller chunks (4MB)
        expected_size: None,
        checksum: None,
        resume: true,
    };

    let downloader = ModelDownloader::new(config);
    downloader.download().await?;

    println!("✅ Download completed for slow network!");
    Ok(())
}

/// Batch download multiple models
pub async fn download_multiple_models() -> Result<()> {
    let models = vec![
        ("https://example.com/model1.gguf", "model1.gguf"),
        ("https://example.com/model2.gguf", "model2.gguf"),
        ("https://example.com/model3.gguf", "model3.gguf"),
    ];

    for (url, filename) in models {
        println!("Downloading {}...", filename);

        let config = DownloadConfig {
            url: url.to_string(),
            output_path: dirs::home_dir()
                .unwrap_or_default()
                .join(".llama")
                .join("models")
                .join(filename),
            parallel_chunks: 4,
            chunk_size: 8 * 1024 * 1024,
            expected_size: None,
            checksum: None,
            resume: true,
        };

        let downloader = ModelDownloader::new(config);
        downloader.download().await?;

        println!("✅ {} downloaded successfully!", filename);
    }

    println!("🎉 All models downloaded!");
    Ok(())
}

/// Example of integrating with LlamaEngine
pub async fn download_and_initialize_llama() -> Result<()> {
    use crate::llm_engine::llama_engine::LlamaEngine;

    // Download the model first
    let model_url = "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf";
    let model_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".llama")
        .join("models")
        .join("tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf");

    let config = DownloadConfig {
        url: model_url.to_string(),
        output_path: model_path.clone(),
        parallel_chunks: 4,
        chunk_size: 8 * 1024 * 1024,
        expected_size: Some(668_066_816),
        checksum: None,
        resume: true,
    };

    let mut downloader = ModelDownloader::new(config);

    // Add progress tracking
    downloader.set_progress_callback(|progress: DownloadProgress| {
        let percentage = progress.percentage * 100.0;
        print!("\rDownloading model: {:.1}%", percentage);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    });

    println!("📥 Downloading Llama model...");
    downloader.download().await?;
    println!("\n✅ Model download completed!");

    // Initialize LlamaEngine with the downloaded model
    println!("🚀 Initializing LlamaEngine...");
    let mut engine = LlamaEngine::with_config(
        model_path.to_string_lossy().to_string(),
        2048, // context size
        35,   // GPU layers
    );

    engine.init().await?;
    println!("✅ LlamaEngine initialized successfully!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // use tempfile::tempdir; // Reserved for future test implementations

    #[tokio::test]
    async fn test_download_config_creation() {
        let config = DownloadConfig {
            url: "https://example.com/test.bin".to_string(),
            output_path: PathBuf::from("/tmp/test.bin"),
            parallel_chunks: 2,
            chunk_size: 1024,
            expected_size: Some(2048),
            checksum: Some("abc123".to_string()),
            resume: true,
        };

        assert_eq!(config.url, "https://example.com/test.bin");
        assert_eq!(config.parallel_chunks, 2);
        assert_eq!(config.chunk_size, 1024);
        assert!(config.resume);
    }
}
