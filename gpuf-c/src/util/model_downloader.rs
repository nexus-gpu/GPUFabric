//! Model downloader with parallel downloading and resume support
//!
//! This module provides functionality to download large model files with:
//! - Parallel chunk downloading for faster speeds
//! - Resume capability for interrupted downloads
//! - Progress tracking and reporting
//! - Integrity verification with checksums

use anyhow::{anyhow, Result};
use reqwest::Client;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

/// Configuration for model downloading
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// URL of the model file to download
    pub url: String,
    /// Local path where the model should be saved
    pub output_path: PathBuf,
    /// Number of parallel download chunks (default: 4)
    pub parallel_chunks: usize,
    /// Chunk size in bytes (default: 8MB)
    pub chunk_size: usize,
    /// Expected file size for verification
    pub expected_size: Option<u64>,
    /// SHA256 checksum for integrity verification
    pub checksum: Option<String>,
    /// Whether to resume interrupted downloads
    pub resume: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            output_path: PathBuf::new(),
            parallel_chunks: 4,
            chunk_size: 8 * 1024 * 1024, // 8MB
            expected_size: None,
            checksum: None,
            resume: true,
        }
    }
}

/// Download progress information
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Total bytes downloaded
    pub downloaded_bytes: u64,
    /// Total file size
    pub total_bytes: u64,
    /// Download percentage (0.0 to 1.0)
    pub percentage: f64,
    /// Download speed in bytes per second
    pub speed_bps: u64,
    /// Estimated time remaining in seconds
    pub eta_seconds: Option<u64>,
}

/// Progress callback type
pub type ProgressCallback = Box<dyn Fn(DownloadProgress) + Send + Sync>;

/// Model downloader with parallel and resume capabilities
pub struct ModelDownloader {
    client: Client,
    config: DownloadConfig,
    progress_callback: Option<Arc<ProgressCallback>>,
}

impl ModelDownloader {
    /// Create a new model downloader with the given configuration
    pub fn new(config: DownloadConfig) -> Self {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .timeout(std::time::Duration::from_secs(300)) // 5 minute timeout
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            progress_callback: None,
        }
    }

    /// Set progress callback for download updates
    pub fn set_progress_callback<F>(&mut self, callback: F)
    where
        F: Fn(DownloadProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(Box::new(callback)));
    }

    /// Start the download with parallel chunks and resume support
    pub async fn download(&self) -> Result<()> {
        info!("Starting download: {}", self.config.url);
        info!("Output path: {:?}", self.config.output_path);

        // Get file info from server
        let file_size = self.get_file_size().await?;
        info!("File size: {} bytes", file_size);

        // If file size is 0, we can't use range requests, fall back to simple download
        if file_size == 0 {
            info!("Server doesn't provide file size, using simple download");
            return self.simple_download().await;
        }

        if let Some(expected) = self.config.expected_size {
            if expected != file_size {
                warn!(
                    "Server file size ({}) differs from expected ({})",
                    file_size, expected
                );
            }
        }

        // Check if we can resume
        let downloaded_size = if self.config.resume && self.config.output_path.exists() {
            self.get_downloaded_size().await?
        } else {
            0
        };

        if downloaded_size > 0 {
            info!(
                "Resuming download: {} bytes already downloaded",
                downloaded_size
            );
        }

        // Create output directory
        if let Some(parent) = self.config.output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Download remaining bytes
        let remaining_bytes = file_size - downloaded_size;
        info!(
            "Downloaded size: {}, File size: {}, Remaining: {}",
            downloaded_size, file_size, remaining_bytes
        );

        if remaining_bytes == 0 {
            info!("File already completely downloaded");
            return Ok(());
        }

        // Calculate chunks
        let chunks = self.calculate_chunks(downloaded_size, remaining_bytes, file_size);
        info!("Downloading {} chunks in parallel", chunks.len());

        // Debug: print chunk info
        for (i, chunk) in chunks.iter().enumerate() {
            info!("Chunk {}: bytes {}-{}", i, chunk.start, chunk.end);
        }

        // Download chunks
        self.download_chunks(chunks, file_size).await?;

        // Verify integrity if checksum provided
        if let Some(checksum) = &self.config.checksum {
            self.verify_checksum(checksum).await?;
        }

        info!("Download completed successfully!");
        Ok(())
    }

    /// Get file size from server headers
    async fn get_file_size(&self) -> Result<u64> {
        // Try HEAD request first
        match self.client.head(&self.config.url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    if let Some(size) = response.content_length() {
                        return Ok(size);
                    }
                }
                // HEAD failed or no content length, try GET
            }
            Err(_) => {
                // HEAD failed, try GET
            }
        }

        // Fallback to GET request with range=0-0 to get just the content length
        let response = self
            .client
            .get(&self.config.url)
            .header("Range", "bytes=0-0")
            .send()
            .await?;

        if !response.status().is_success() && response.status() != 206 {
            return Err(anyhow!("Failed to get file info: {}", response.status()));
        }

        response
            .content_length()
            .ok_or_else(|| anyhow!("Server didn't provide content length"))
    }

    /// Get current size of partially downloaded file
    async fn get_downloaded_size(&self) -> Result<u64> {
        let metadata = tokio::fs::metadata(&self.config.output_path).await?;
        Ok(metadata.len())
    }

    /// Calculate download chunks for parallel downloading
    fn calculate_chunks(
        &self,
        start_pos: u64,
        remaining: u64,
        total_size: u64,
    ) -> Vec<DownloadChunk> {
        let mut chunks = Vec::new();
        let chunk_size = self.config.chunk_size as u64;

        // If file is small, use single chunk
        if remaining <= chunk_size || self.config.parallel_chunks == 1 {
            chunks.push(DownloadChunk {
                start: start_pos,
                end: total_size - 1,
                index: 0,
            });
            return chunks;
        }

        // Calculate optimal chunk count
        let chunk_count = (remaining / chunk_size).min(self.config.parallel_chunks as u64) as usize;
        let actual_chunk_size = remaining / chunk_count as u64;

        for i in 0..chunk_count {
            let chunk_start = start_pos + (i as u64 * actual_chunk_size);
            let chunk_end = if i == chunk_count - 1 {
                total_size - 1
            } else {
                chunk_start + actual_chunk_size - 1
            };

            chunks.push(DownloadChunk {
                start: chunk_start,
                end: chunk_end,
                index: i,
            });
        }

        chunks
    }

    /// Download multiple chunks in parallel
    async fn download_chunks(&self, chunks: Vec<DownloadChunk>, total_size: u64) -> Result<()> {
        let semaphore = Arc::new(Semaphore::new(self.config.parallel_chunks));
        let downloaded_bytes = Arc::new(Mutex::new(0u64));
        let start_time = std::time::Instant::now();
        let total_chunks = chunks.len();

        let mut set = JoinSet::new();

        for chunk in chunks {
            let semaphore = semaphore.clone();
            let client = self.client.clone();
            let url = self.config.url.clone();
            let output_path = self.config.output_path.clone();
            let downloaded_bytes = downloaded_bytes.clone();
            let progress_callback = self.progress_callback.clone();

            set.spawn(async move {
                let _permit = semaphore.acquire().await?;

                let result = Self::download_chunk(
                    client,
                    &url,
                    &output_path,
                    chunk,
                    downloaded_bytes.clone(),
                    total_size,
                    progress_callback,
                    start_time,
                )
                .await;

                // Return the chunk index for error reporting
                match result {
                    Ok(_) => Ok(chunk.index),
                    Err(e) => {
                        error!("Failed to download chunk {}: {}", chunk.index, e);
                        Err(e)
                    }
                }
            });
        }

        // Wait for all chunks to complete
        let mut completed = 0;
        while let Some(result) = set.join_next().await {
            match result {
                Ok(Ok(chunk_index)) => {
                    completed += 1;
                    debug!(
                        "Chunk {} completed ({} / {})",
                        chunk_index, completed, total_chunks
                    );
                }
                Ok(Err(e)) => {
                    return Err(anyhow!("Chunk download failed: {}", e));
                }
                Err(e) => {
                    return Err(anyhow!("Task join error: {}", e));
                }
            }
        }

        Ok(())
    }

    /// Download a single chunk with range request
    async fn download_chunk(
        client: Client,
        url: &str,
        output_path: &Path,
        chunk: DownloadChunk,
        downloaded_bytes: Arc<Mutex<u64>>,
        total_size: u64,
        progress_callback: Option<Arc<ProgressCallback>>,
        start_time: std::time::Instant,
    ) -> Result<()> {
        let range_header = format!("bytes={}-{}", chunk.start, chunk.end);

        let response = client.get(url).header("Range", range_header).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Chunk download failed: {}", response.status()));
        }

        let chunk_data = response.bytes().await?;

        // Write chunk to file at correct position
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(output_path)?;

        file.seek(SeekFrom::Start(chunk.start))?;
        file.write_all(&chunk_data)?;
        file.sync_all()?;

        // Update progress
        {
            let mut downloaded = downloaded_bytes.lock().await;
            *downloaded += chunk_data.len() as u64;

            if let Some(callback) = progress_callback {
                let progress = DownloadProgress {
                    downloaded_bytes: *downloaded,
                    total_bytes: total_size,
                    percentage: (*downloaded as f64) / (total_size as f64),
                    speed_bps: if start_time.elapsed().as_secs() > 0 {
                        (*downloaded) / start_time.elapsed().as_secs()
                    } else {
                        0
                    },
                    eta_seconds: if start_time.elapsed().as_secs() > 0 {
                        let avg_speed = (*downloaded) / start_time.elapsed().as_secs();
                        if avg_speed > 0 {
                            Some((total_size - *downloaded) / avg_speed)
                        } else {
                            None
                        }
                    } else {
                        None
                    },
                };

                callback(progress);
            }
        }

        Ok(())
    }

    /// Verify file integrity using SHA256 checksum
    async fn verify_checksum(&self, expected_checksum: &str) -> Result<()> {
        use sha2::{Digest, Sha256};

        info!("Verifying file integrity...");

        let mut file = tokio::fs::File::open(&self.config.output_path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 8192];

        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let actual_checksum = format!("{:x}", hasher.finalize());

        if actual_checksum.to_lowercase() != expected_checksum.to_lowercase() {
            return Err(anyhow!(
                "Checksum verification failed. Expected: {}, Actual: {}",
                expected_checksum,
                actual_checksum
            ));
        }

        info!("Checksum verification passed");
        Ok(())
    }

    /// Simple download for servers that don't provide Content-Length
    async fn simple_download(&self) -> Result<()> {
        info!("Starting simple download (no Content-Length)");

        let response = self.client.get(&self.config.url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Download failed: {}", response.status()));
        }

        let total_size = response.content_length().unwrap_or(0);
        info!("Actual content length: {} bytes", total_size);

        // Create output directory
        if let Some(parent) = self.config.output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = tokio::fs::File::create(&self.config.output_path).await?;
        let mut downloaded_bytes = 0u64;
        let start_time = std::time::Instant::now();

        // Use streaming download
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            file.write_all(&chunk).await?;
            downloaded_bytes += chunk.len() as u64;

            // Update progress
            if let Some(callback) = &self.progress_callback {
                let progress = DownloadProgress {
                    downloaded_bytes,
                    total_bytes: if total_size > 0 {
                        total_size
                    } else {
                        downloaded_bytes
                    },
                    percentage: if total_size > 0 {
                        (downloaded_bytes as f64) / (total_size as f64)
                    } else {
                        0.0 // Can't calculate percentage without total size
                    },
                    speed_bps: if start_time.elapsed().as_secs() > 0 {
                        downloaded_bytes / start_time.elapsed().as_secs()
                    } else {
                        0
                    },
                    eta_seconds: if total_size > 0 && start_time.elapsed().as_secs() > 0 {
                        let avg_speed = downloaded_bytes / start_time.elapsed().as_secs();
                        if avg_speed > 0 {
                            Some((total_size - downloaded_bytes) / avg_speed)
                        } else {
                            None
                        }
                    } else {
                        None
                    },
                };

                callback(progress);
            }
        }

        file.sync_all().await?;
        info!("Simple download completed: {} bytes", downloaded_bytes);

        // Verify integrity if checksum provided
        if let Some(checksum) = &self.config.checksum {
            self.verify_checksum(checksum).await?;
        }

        Ok(())
    }
}

/// Represents a download chunk
#[derive(Debug, Clone, Copy)]
struct DownloadChunk {
    start: u64,
    end: u64,
    index: usize,
}

/// Convenience function for simple downloads
pub async fn download_model(url: &str, output_path: &Path) -> Result<()> {
    let config = DownloadConfig {
        url: url.to_string(),
        output_path: output_path.to_path_buf(),
        ..Default::default()
    };

    let downloader = ModelDownloader::new(config);
    downloader.download().await
}

/// Convenience function with progress callback
pub async fn download_model_with_progress(
    url: &str,
    output_path: &Path,
    progress_callback: impl Fn(DownloadProgress) + Send + Sync + 'static,
) -> Result<()> {
    let config = DownloadConfig {
        url: url.to_string(),
        output_path: output_path.to_path_buf(),
        ..Default::default()
    };

    let mut downloader = ModelDownloader::new(config);
    downloader.set_progress_callback(progress_callback);
    downloader.download().await
}

#[cfg(test)]
mod tests {
    use super::*;
    // use tempfile::tempdir; // Reserved for future test implementations

    #[tokio::test]
    async fn test_download_config_default() {
        let config = DownloadConfig::default();
        assert_eq!(config.parallel_chunks, 4);
        assert_eq!(config.chunk_size, 8 * 1024 * 1024);
        assert!(config.resume);
    }

    #[tokio::test]
    async fn test_chunk_calculation() {
        let downloader = ModelDownloader::new(DownloadConfig {
            parallel_chunks: 4,
            chunk_size: 1024,
            ..Default::default()
        });

        // Test small file (single chunk)
        let chunks = downloader.calculate_chunks(0, 500, 500);
        assert_eq!(chunks.len(), 1);

        // Test large file (multiple chunks)
        let chunks = downloader.calculate_chunks(0, 5000, 5000);
        assert_eq!(chunks.len(), 4);
    }
}
