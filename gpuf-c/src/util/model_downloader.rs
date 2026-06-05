//! Model downloader with parallel downloading and resume support
//!
//! This module provides functionality to download large model files with:
//! - Parallel chunk downloading for faster speeds
//! - Resume capability for interrupted downloads
//! - Progress tracking and reporting
//! - Integrity verification with checksums

use crate::util::security_metrics;
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{timeout, Duration};
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
    /// Required SHA256 checksum for integrity verification
    pub checksum: String,
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
            checksum: String::new(),
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
        self.validate_config().await?;

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
        let mut downloaded_size = if self.config.resume && self.config.output_path.exists() {
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

        if downloaded_size > file_size {
            warn!(
                "Existing file is larger than server file size (existing={}, server={}), re-downloading",
                downloaded_size, file_size
            );
            self.safe_remove_output_file().await?;
            let _ = tokio::fs::remove_dir_all(self.parts_dir()).await;
            downloaded_size = 0;
        } else if downloaded_size == file_size && file_size > 0 {
            match self.verify_checksum(&self.config.checksum).await {
                Ok(()) => {
                    info!("File already completely downloaded and checksum verified");
                    return Ok(());
                }
                Err(e) => {
                    warn!(
                        "Existing file matches server size but checksum failed ({}), re-downloading",
                        e
                    );
                    self.safe_remove_output_file().await?;
                    let _ = tokio::fs::remove_dir_all(self.parts_dir()).await;
                    downloaded_size = 0;
                }
            }
        }

        if downloaded_size > 0 {
            info!(
                "Resume detected ({} bytes already present). Using sequential ranged download to avoid file corruption.",
                downloaded_size
            );
            return self.simple_download().await;
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
        self.download_chunks(chunks, file_size, downloaded_size)
            .await?;

        self.verify_checksum(&self.config.checksum).await?;

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
                        if size > 0 {
                            return Ok(size);
                        }
                    }
                }
                // HEAD failed or no content length, try GET with Range
            }
            Err(_) => {
                // HEAD failed, try GET with Range
            }
        }

        // Fallback to GET request with range=0-0 to get file size from Content-Range header
        // Response will be: Content-Range: bytes 0-0/TOTAL_SIZE
        let response = self
            .client
            .get(&self.config.url)
            .header("Range", "bytes=0-0")
            .send()
            .await?;

        // Check for 206 Partial Content (range request supported)
        if response.status() == 206 {
            // Parse Content-Range header: "bytes 0-0/TOTAL_SIZE"
            if let Some(content_range) = response.headers().get("content-range") {
                if let Ok(range_str) = content_range.to_str() {
                    // Format: "bytes 0-0/26883306112"
                    if let Some(total_size_str) = range_str.split('/').last() {
                        if let Ok(total_size) = total_size_str.parse::<u64>() {
                            return Ok(total_size);
                        }
                    }
                }
            }
        }

        // If range request not supported, try content_length from response
        if response.status().is_success() {
            if let Some(size) = response.content_length() {
                if size > 0 {
                    return Ok(size);
                }
            }
        }

        // Server doesn't provide file size
        Ok(0)
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
    async fn download_chunks(
        &self,
        chunks: Vec<DownloadChunk>,
        total_size: u64,
        initial_downloaded: u64,
    ) -> Result<()> {
        let parts_dir = self.parts_dir();
        tokio::fs::create_dir_all(&parts_dir).await?;

        let mut existing = 0u64;
        for chunk in chunks.iter() {
            let part_path = Self::part_path(&parts_dir, chunk.index);
            if let Ok(meta) = tokio::fs::metadata(&part_path).await {
                let len = meta.len();
                let max_len = (chunk.end - chunk.start) + 1;
                if len <= max_len {
                    existing += len;
                } else {
                    let _ = tokio::fs::remove_file(&part_path).await;
                }
            }
        }

        let semaphore = Arc::new(Semaphore::new(self.config.parallel_chunks));
        let baseline_downloaded = initial_downloaded + existing;
        let downloaded_bytes = Arc::new(Mutex::new(baseline_downloaded));
        let start_time = std::time::Instant::now();
        let total_chunks = chunks.len();

        let mut set = JoinSet::new();

        for chunk in chunks {
            let semaphore = semaphore.clone();
            let client = self.client.clone();
            let url = self.config.url.clone();
            let output_path = self.config.output_path.clone();
            let parts_dir = parts_dir.clone();
            let downloaded_bytes = downloaded_bytes.clone();
            let progress_callback = self.progress_callback.clone();
            let baseline_downloaded = baseline_downloaded;

            set.spawn(async move {
                let _permit = semaphore.acquire().await?;

                let result = Self::download_chunk_to_part(
                    client,
                    &url,
                    &output_path,
                    &parts_dir,
                    chunk,
                    downloaded_bytes.clone(),
                    total_size,
                    progress_callback,
                    start_time,
                    baseline_downloaded,
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

        let temp_path = self.temp_output_path();
        let _ = tokio::fs::remove_file(&temp_path).await;
        Self::assemble_parts(&parts_dir, &temp_path, total_chunks).await?;
        Self::verify_checksum_at_path(&temp_path, &self.config.checksum).await?;
        tokio::fs::rename(&temp_path, &self.config.output_path).await?;

        let _ = tokio::fs::remove_dir_all(&parts_dir).await;

        Ok(())
    }

    fn parts_dir(&self) -> PathBuf {
        let mut p = self.config.output_path.to_string_lossy().to_string();
        p.push_str(".parts");
        PathBuf::from(p)
    }

    fn part_path(parts_dir: &Path, index: usize) -> PathBuf {
        parts_dir.join(format!("part-{}", index))
    }

    fn temp_output_path(&self) -> PathBuf {
        let pid = std::process::id();
        let file_name = self
            .config
            .output_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("model");
        self.config
            .output_path
            .with_file_name(format!("{}.tmp.{}", file_name, pid))
    }

    async fn validate_config(&self) -> Result<()> {
        Self::normalize_sha256(&self.config.checksum)?;
        Self::validate_output_path(&self.config.output_path).await?;
        Ok(())
    }

    fn normalize_sha256(raw: &str) -> Result<String> {
        let checksum = raw
            .strip_prefix("sha256:")
            .unwrap_or(raw)
            .trim()
            .to_ascii_lowercase();
        if checksum.len() != 64 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(anyhow!("Model download requires a valid SHA256 checksum"));
        }
        Ok(checksum)
    }

    async fn validate_output_path(output_path: &Path) -> Result<()> {
        if output_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(anyhow!("Model output path must not contain '..'"));
        }
        let Some(parent) = output_path.parent() else {
            return Err(anyhow!("Model output path must have a parent directory"));
        };
        tokio::fs::create_dir_all(parent).await?;
        let canonical_parent = parent.canonicalize()?;
        let file_name = output_path
            .file_name()
            .ok_or_else(|| anyhow!("Model output path must include a file name"))?;
        let checked_path = canonical_parent.join(file_name);
        if checked_path != output_path {
            if output_path.is_absolute() {
                let normalized = output_path
                    .parent()
                    .and_then(|p| p.canonicalize().ok())
                    .map(|p| p.join(file_name));
                if normalized.as_deref() != Some(&checked_path) {
                    return Err(anyhow!("Model output path escapes its parent directory"));
                }
            }
        }
        let ext = checked_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "gguf" | "bin" | "safetensors") {
            return Err(anyhow!(
                "Model output path must end with .gguf, .bin, or .safetensors"
            ));
        }
        Ok(())
    }

    async fn safe_remove_output_file(&self) -> Result<()> {
        let Some(parent) = self.config.output_path.parent() else {
            return Err(anyhow!("Model output path must have a parent directory"));
        };
        let canonical_parent = parent.canonicalize()?;
        let file_name = self
            .config
            .output_path
            .file_name()
            .ok_or_else(|| anyhow!("Model output path must include a file name"))?;
        let safe_path = canonical_parent.join(file_name);
        if let Ok(meta) = tokio::fs::symlink_metadata(&safe_path).await {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(anyhow!("Refusing to remove unsafe model output path"));
            }
            tokio::fs::remove_file(safe_path).await?;
        }
        Ok(())
    }

    async fn assemble_parts(
        parts_dir: &Path,
        output_path: &Path,
        total_parts: usize,
    ) -> Result<()> {
        let mut out = tokio::fs::File::create(output_path).await?;
        for i in 0..total_parts {
            let part = Self::part_path(parts_dir, i);
            let mut f = tokio::fs::File::open(&part).await?;
            tokio::io::copy(&mut f, &mut out).await?;
        }
        out.sync_all().await?;
        Ok(())
    }

    async fn download_chunk_to_part(
        client: Client,
        url: &str,
        output_path: &Path,
        parts_dir: &Path,
        chunk: DownloadChunk,
        downloaded_bytes: Arc<Mutex<u64>>,
        total_size: u64,
        progress_callback: Option<Arc<ProgressCallback>>,
        start_time: std::time::Instant,
        baseline_downloaded: u64,
    ) -> Result<()> {
        let _ = output_path;
        let part_path = Self::part_path(parts_dir, chunk.index);

        let existing_len = match tokio::fs::metadata(&part_path).await {
            Ok(meta) => meta.len(),
            Err(_) => 0,
        };

        let max_len = (chunk.end - chunk.start) + 1;
        let existing_len = existing_len.min(max_len);
        let start = chunk.start + existing_len;
        if start > chunk.end {
            return Ok(());
        }

        let range_header = format!("bytes={}-{}", start, chunk.end);
        let response = client.get(url).header("Range", range_header).send().await?;

        if response.status() != 206 {
            return Err(anyhow!(
                "Server did not honor Range request for chunk {} (status: {})",
                chunk.index,
                response.status()
            ));
        }

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open(&part_path)
            .await?;

        let mut stream = response.bytes_stream();
        let mut last_report = std::time::Instant::now();

        loop {
            let next = timeout(Duration::from_secs(30), stream.next()).await;
            match next {
                Ok(Some(item)) => {
                    let bytes = item?;
                    file.write_all(&bytes).await?;

                    let mut downloaded = downloaded_bytes.lock().await;
                    *downloaded += bytes.len() as u64;

                    if let Some(callback) = progress_callback.as_ref() {
                        if last_report.elapsed().as_secs() >= 1 {
                            last_report = std::time::Instant::now();
                            let elapsed_secs = start_time.elapsed().as_secs();
                            let downloaded_since_start =
                                downloaded.saturating_sub(baseline_downloaded);
                            let speed_bps = if elapsed_secs > 0 {
                                downloaded_since_start / elapsed_secs
                            } else {
                                0
                            };
                            let progress = DownloadProgress {
                                downloaded_bytes: *downloaded,
                                total_bytes: total_size,
                                percentage: (*downloaded as f64) / (total_size as f64),
                                speed_bps,
                                eta_seconds: if speed_bps > 0 {
                                    Some((total_size - *downloaded) / speed_bps)
                                } else {
                                    None
                                },
                            };
                            callback(progress);
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    return Err(anyhow!("Chunk download stalled (timeout waiting for data)"));
                }
            }
        }

        file.flush().await?;

        Ok(())
    }

    /// Verify file integrity using SHA256 checksum
    async fn verify_checksum(&self, expected_checksum: &str) -> Result<()> {
        Self::verify_checksum_at_path(&self.config.output_path, expected_checksum).await
    }

    async fn verify_checksum_at_path(path: &Path, expected_checksum: &str) -> Result<()> {
        use sha2::{Digest, Sha256};

        info!("Verifying file integrity...");
        let expected_checksum = Self::normalize_sha256(expected_checksum)?;

        let mut file = tokio::fs::File::open(path).await?;
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

        if actual_checksum != expected_checksum {
            security_metrics::record_checksum_failure();
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

        // Check if we can resume from existing file
        let resume_from = if self.config.resume && self.config.output_path.exists() {
            let metadata = tokio::fs::metadata(&self.config.output_path).await?;
            let size = metadata.len();
            info!("Found existing file with {} bytes, attempting resume", size);
            size
        } else {
            0
        };

        // Send request with Range header if resuming
        let response = if resume_from > 0 {
            self.client
                .get(&self.config.url)
                .header("Range", format!("bytes={}-", resume_from))
                .send()
                .await?
        } else {
            self.client.get(&self.config.url).send().await?
        };

        if response.status() == 416 && resume_from > 0 {
            self.verify_checksum(&self.config.checksum).await?;
            info!("Range resume not satisfiable; treating existing file as complete");
            return Ok(());
        }

        if !response.status().is_success() && response.status() != 206 {
            return Err(anyhow!("Download failed: {}", response.status()));
        }

        let effective_resume_from = if resume_from > 0 && response.status() == 206 {
            resume_from
        } else {
            0
        };

        let total_size = response.content_length().unwrap_or(0);
        let actual_total = if effective_resume_from > 0 {
            effective_resume_from + total_size
        } else {
            total_size
        };
        info!(
            "Actual content length: {} bytes (resume from: {})",
            actual_total, effective_resume_from
        );

        // Create output directory
        if let Some(parent) = self.config.output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let temp_path = self.temp_output_path();
        if effective_resume_from == 0 {
            let _ = tokio::fs::remove_file(&temp_path).await;
        }

        // Open temp file for append if resuming, otherwise create new.
        let mut file = if effective_resume_from > 0 && temp_path.exists() {
            use tokio::fs::OpenOptions;
            OpenOptions::new()
                .write(true)
                .append(true)
                .open(&temp_path)
                .await?
        } else {
            tokio::fs::File::create(&temp_path).await?
        };

        let mut downloaded_bytes = effective_resume_from;
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
                    total_bytes: if actual_total > 0 {
                        actual_total
                    } else {
                        downloaded_bytes
                    },
                    percentage: if actual_total > 0 {
                        (downloaded_bytes as f64) / (actual_total as f64)
                    } else {
                        0.0 // Can't calculate percentage without total size
                    },
                    speed_bps: if start_time.elapsed().as_secs() > 0 {
                        (downloaded_bytes - effective_resume_from) / start_time.elapsed().as_secs()
                    } else {
                        0
                    },
                    eta_seconds: if actual_total > 0 && start_time.elapsed().as_secs() > 0 {
                        let avg_speed = (downloaded_bytes - effective_resume_from)
                            / start_time.elapsed().as_secs();
                        if avg_speed > 0 {
                            Some((actual_total - downloaded_bytes) / avg_speed)
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

        Self::verify_checksum_at_path(&temp_path, &self.config.checksum).await?;
        tokio::fs::rename(&temp_path, &self.config.output_path).await?;

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
pub async fn download_model(url: &str, output_path: &Path, checksum: &str) -> Result<()> {
    let config = DownloadConfig {
        url: url.to_string(),
        output_path: output_path.to_path_buf(),
        checksum: checksum.to_string(),
        ..Default::default()
    };

    let downloader = ModelDownloader::new(config);
    downloader.download().await
}

/// Convenience function with progress callback
pub async fn download_model_with_progress(
    url: &str,
    output_path: &Path,
    checksum: &str,
    progress_callback: impl Fn(DownloadProgress) + Send + Sync + 'static,
) -> Result<()> {
    let config = DownloadConfig {
        url: url.to_string(),
        output_path: output_path.to_path_buf(),
        checksum: checksum.to_string(),
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
    async fn test_checksum_validation_requires_sha256() {
        assert!(ModelDownloader::normalize_sha256("").is_err());
        assert!(ModelDownloader::normalize_sha256("abc123").is_err());
        assert!(ModelDownloader::normalize_sha256(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        )
        .is_ok());
    }

    #[tokio::test]
    async fn test_output_path_validation_rejects_unsafe_paths() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let safe = temp.path().join("model.gguf");
        ModelDownloader::validate_output_path(&safe).await?;

        let bad_ext = temp.path().join("model.txt");
        assert!(ModelDownloader::validate_output_path(&bad_ext)
            .await
            .is_err());

        let outside = temp.path().join("..").join("model.gguf");
        assert!(ModelDownloader::validate_output_path(&outside)
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_verify_checksum_at_path() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let model = temp.path().join("model.gguf");
        tokio::fs::write(&model, b"abc").await?;

        ModelDownloader::verify_checksum_at_path(
            &model,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        .await?;
        assert!(ModelDownloader::verify_checksum_at_path(
            &model,
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .await
        .is_err());
        Ok(())
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
