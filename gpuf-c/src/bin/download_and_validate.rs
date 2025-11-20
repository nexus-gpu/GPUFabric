//! Download and Validate Model Tool
//! 
//! This tool downloads a model file and validates its format

use anyhow::Result;
use std::path::PathBuf;
use std::io::Read;
use clap::{Arg, Command};

use gpuf_c::util::model_downloader::{ModelDownloader, DownloadConfig, DownloadProgress};

#[tokio::main]
async fn main() -> Result<()> {
    let matches = Command::new("download_and_validate")
        .version("1.0")
        .about("Download model file and validate its format")
        .arg(
            Arg::new("url")
                .help("URL of the model file to download")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("output")
                .help("Output path for the downloaded file")
                .index(2),
        )
        .arg(
            Arg::new("chunks")
                .long("chunks")
                .short('c')
                .help("Number of parallel download chunks")
                .value_parser(clap::value_parser!(usize))
                .default_value("4"),
        )
        .arg(
            Arg::new("chunk-size")
                .long("chunk-size")
                .short('s')
                .help("Chunk size in MB")
                .value_parser(clap::value_parser!(usize))
                .default_value("8"),
        )
        .arg(
            Arg::new("checksum")
                .long("checksum")
                .short('x')
                .help("SHA256 checksum for verification"),
        )
        .arg(
            Arg::new("no-resume")
                .long("no-resume")
                .help("Disable resume functionality")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("validate")
                .long("validate")
                .short('v')
                .help("Validate file format after download")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let url = matches.get_one::<String>("url").unwrap();
    let output_path = matches
        .get_one::<String>("output")
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(|| {
            // Extract filename from URL
            let filename = url
                .split('/')
                .last()
                .unwrap_or("downloaded_model.bin");
            PathBuf::from(filename)
        });

    let parallel_chunks = *matches.get_one::<usize>("chunks").unwrap();
    let chunk_size_mb = *matches.get_one::<usize>("chunk-size").unwrap();
    let checksum = matches.get_one::<String>("checksum").cloned();
    let resume = !matches.get_flag("no-resume");
    let validate = matches.get_flag("validate");

    println!("🚀 GPUFabric Download & Validate Tool");
    println!("📥 URL: {}", url);
    println!("💾 Output: {:?}", output_path);
    println!("🔧 Parallel chunks: {}", parallel_chunks);
    println!("📦 Chunk size: {} MB", chunk_size_mb);
    println!("🔄 Resume: {}", if resume { "Enabled" } else { "Disabled" });
    if checksum.is_some() {
        println!("🔐 Checksum verification: Enabled");
    }
    println!("🔍 Validation: {}", if validate { "Enabled" } else { "Disabled" });
    println!();

    // Download the file
    let config = DownloadConfig {
        url: url.clone(),
        output_path: output_path.clone(),
        parallel_chunks,
        chunk_size: chunk_size_mb * 1024 * 1024,
        expected_size: None,
        checksum,
        resume,
    };

    let mut downloader = ModelDownloader::new(config);
    
    // Set up progress tracking
    let start_time = std::time::Instant::now();
    downloader.set_progress_callback(move |progress: DownloadProgress| {
        let percentage = progress.percentage * 100.0;
        let downloaded_mb = progress.downloaded_bytes / (1024 * 1024);
        let total_mb = progress.total_bytes / (1024 * 1024);
        let speed_mbps = progress.speed_bps / (1024 * 1024);
        
        // Clear line and print progress
        print!(
            "\r⏳ Progress: {:.1}% ({}/{} MB) - {:.1} MB/s",
            percentage, downloaded_mb, total_mb, speed_mbps
        );
        
        if let Some(eta) = progress.eta_seconds {
            let eta_minutes = eta / 60;
            let eta_seconds = eta % 60;
            print!(" - ETA: {}:{:02}", eta_minutes, eta_seconds);
        }
        
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    });

    println!("🔄 Starting download...");
    match downloader.download().await {
        Ok(_) => {
            println!();
            println!("✅ Download completed successfully!");
            
            // Show file info
            match std::fs::metadata(&output_path) {
                Ok(metadata) => {
                    let file_size_mb = metadata.len() / (1024 * 1024);
                    let elapsed_seconds = start_time.elapsed().as_secs();
                    let avg_speed_mbps = if elapsed_seconds > 0 {
                        metadata.len() / (1024 * 1024) / elapsed_seconds
                    } else {
                        0
                    };
                    
                    println!("📊 File size: {} MB", file_size_mb);
                    println!("⏱️  Time elapsed: {} seconds", elapsed_seconds);
                    println!("📈 Average speed: {} MB/s", avg_speed_mbps);
                    println!("💾 File saved to: {:?}", output_path);
                }
                Err(e) => {
                    println!("⚠️  Warning: Could not get file metadata: {}", e);
                    println!("💾 Expected file location: {:?}", output_path);
                }
            }

            // Validate file format if requested
            if validate {
                println!();
                println!("🔍 Validating file format...");
                
                if let Err(e) = validate_file_format(&output_path) {
                    println!("❌ Validation failed: {}", e);
                } else {
                    println!("✅ File format validation passed!");
                }
            }
        }
        Err(e) => {
            println!();
            eprintln!("❌ Download failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn validate_file_format(file_path: &PathBuf) -> Result<()> {
    // Check if file exists
    if !file_path.exists() {
        return Err(anyhow::anyhow!("File does not exist: {:?}", file_path));
    }

    // Get file size
    let metadata = std::fs::metadata(file_path)?;
    let file_size = metadata.len();
    
    if file_size == 0 {
        return Err(anyhow::anyhow!("File is empty"));
    }

    // Open and read file
    let mut file = std::fs::File::open(file_path)?;
    
    // Read first few bytes to determine format
    let mut header = [0u8; 8];
    file.read_exact(&mut header)?;
    
    // Check for different model formats
    if header.starts_with(b"GGUF") {
        println!("🤖 Detected format: GGUF (GPT-Generated Unified Format)");
        validate_gguf_format(file_path)?;
    } else if header.starts_with(b"PK") {
        println!("📦 Detected format: ZIP/Safetensors archive");
        println!("✅ Valid ZIP archive format");
    } else if header.starts_with(b"\x89PNG") {
        println!("🖼️  Detected format: PNG image");
        println!("⚠️  Warning: This appears to be an image file, not a model");
    } else if header.starts_with(b"<?xml") || header.starts_with(b"<html") {
        println!("🌐 Detected format: HTML/XML");
        println!("⚠️  Warning: This appears to be a web page, not a model");
    } else if header.starts_with(b"%PDF") {
        println!("📄 Detected format: PDF document");
        println!("⚠️  Warning: This appears to be a PDF file, not a model");
    } else {
        println!("❓ Unknown format: {:?}", &header[..4]);
        println!("⚠️  Could not determine file format");
    }

    Ok(())
}

fn validate_gguf_format(file_path: &PathBuf) -> Result<()> {
    use std::io::Read;
    
    let mut file = std::fs::File::open(file_path)?;
    
    // Read magic number (first 4 bytes)
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    
    // Check GGUF magic number
    let expected_magic = b"GGUF";
    if magic != *expected_magic {
        return Err(anyhow::anyhow!("Invalid GGUF magic number"));
    }

    // Read version (next 4 bytes)
    let mut version_bytes = [0u8; 4];
    file.read_exact(&mut version_bytes)?;
    let version = u32::from_le_bytes(version_bytes);
    
    if version < 1 || version > 3 {
        return Err(anyhow::anyhow!("Unsupported GGUF version: {}", version));
    }

    // Read tensor count (next 8 bytes)
    let mut tensor_count_bytes = [0u8; 8];
    file.read_exact(&mut tensor_count_bytes)?;
    let tensor_count = u64::from_le_bytes(tensor_count_bytes);
    
    if tensor_count == 0 {
        return Err(anyhow::anyhow!("Invalid tensor count: 0"));
    }

    // Read KV count (next 8 bytes)
    let mut kv_count_bytes = [0u8; 8];
    file.read_exact(&mut kv_count_bytes)?;
    let kv_count = u64::from_le_bytes(kv_count_bytes);
    
    if kv_count == 0 {
        return Err(anyhow::anyhow!("Invalid KV count: 0"));
    }

    println!("📋 GGUF version: {}", version);
    println!("🧩 Tensor count: {}", tensor_count);
    println!("🔑 KV count: {}", kv_count);
    println!("✅ Valid GGUF structure");

    Ok(())
}
