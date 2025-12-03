use super::Engine;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::fs;
use tracing::{debug, info, warn};

#[allow(dead_code)] // LLM engine implementation for llama.cpp (embedded mode)
#[derive(Clone)] // Enable cloning for shared instance usage
pub struct LlamaEngine {
    pub models: Arc<RwLock<Vec<super::ModelInfo>>>,
    pub models_name: Vec<String>,
    pub model_path: Option<String>,
    pub n_ctx: u32,
    pub n_gpu_layers: u32,
    pub is_initialized: bool,
    pub models_dir: PathBuf,
    // Added: model loading status tracking
    pub loading_status: Arc<RwLock<String>>, // "not_loaded", "loading", "loaded", "error"
    pub current_loading_model: Arc<RwLock<Option<String>>>,
}

#[allow(dead_code)] // LlamaEngine implementation methods
impl LlamaEngine {
    pub fn new() -> Self {
        let models_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".llama")
            .join("models");
        
        LlamaEngine {
            models: Arc::new(RwLock::new(Vec::new())),
            models_name: Vec::new(),
            model_path: None,
            n_ctx: 2048,
            n_gpu_layers: 0,
            is_initialized: false,
            models_dir,
            loading_status: Arc::new(RwLock::new("not_loaded".to_string())),
            current_loading_model: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_config(model_path: String, n_ctx: u32, n_gpu_layers: u32) -> Self {
        let models_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".llama")
            .join("models");
        
        LlamaEngine {
            models: Arc::new(RwLock::new(Vec::new())),
            models_name: Vec::new(),
            model_path: Some(model_path.clone()),
            n_ctx,
            n_gpu_layers,
            is_initialized: false,
            models_dir,
            loading_status: Arc::new(RwLock::new("not_loaded".to_string())),
            current_loading_model: Arc::new(RwLock::new(None)),
        }
    }

    async fn ensure_initialized(&mut self) -> Result<()> {
        if !self.is_initialized {
            if let Some(model_path) = &self.model_path {
                info!("Initializing Llama.cpp engine with model: {}", model_path);
                // Simulate engine initialization
                if std::path::Path::new(model_path).exists() {
                    self.is_initialized = true;
                    info!("Llama.cpp engine initialized successfully (simulated)");
                } else {
                    return Err(anyhow!("Model file not found: {}", model_path));
                }
            } else {
                return Err(anyhow!("Model path not set for Llama.cpp engine"));
            }
        }
        Ok(())
    }

    fn validate_model_path(&self, path: &str) -> Result<PathBuf> {
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            return Err(anyhow!("Model file does not exist: {}", path));
        }
        if !path_buf.is_file() {
            return Err(anyhow!("Model path is not a file: {}", path));
        }
        Ok(path_buf)
    }

    async fn generate_response(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        if !self.is_initialized {
            return Err(anyhow!("Llama.cpp engine is not initialized"));
        }

        debug!("Generating response with prompt: {}, max_tokens: {}", prompt, max_tokens);
        // Simulate text generation
        Ok(format!("Generated response for: {} (simulated, {} tokens)", &prompt[..prompt.len().min(30)], max_tokens))
    }
}

impl Engine for LlamaEngine {
    fn init(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Initializing Llama.cpp engine");
            
            if self.model_path.is_none() {
                warn!("No model path specified, engine will be initialized when model is set");
                return Ok(());
            }

            self.ensure_initialized().await?;
            Ok(())
        }
    }

    fn set_models(&mut self, models: Vec<String>) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Setting models for Llama.cpp engine: {:?}", models);
            
            if models.is_empty() {
                return Err(anyhow!("At least one model must be specified"));
            }

            // For Llama.cpp, we only support one model at a time
            let model_path = models[0].clone();
            
            // Validate model path
            self.validate_model_path(&model_path)?;
            
            // If engine is already initialized with a different model, unload it first
            if self.is_initialized {
                if Some(model_path.clone()) != self.model_path {
                    info!("Unloading previous model before loading new one");
                    // Simulate engine unload
                    self.is_initialized = false;
                    info!("Previous model unloaded (simulated)");
                }
            }

            // Update model configuration
            self.model_path = Some(model_path.clone());
            self.models_name = vec![model_path.clone()];
            
            // Initialize with new model
            self.ensure_initialized().await?;
            
            // Update models list
            let mut models_vec = self.models.write().await;
            models_vec.clear();
            models_vec.push(super::ModelInfo {
                id: "llama_cpp_model".to_string(),
                name: model_path,
                status: "loaded".to_string(),
            });

            info!("Models set successfully for Llama.cpp engine");
            Ok(())
        }
    }

    fn start_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Starting Llama.cpp worker");
            
            // For Llama.cpp, the "worker" is essentially just ensuring the engine is initialized
            self.ensure_initialized().await?;
            
            info!("Llama.cpp worker started successfully");
            Ok(())
        }
    }

    fn stop_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Stopping Llama.cpp worker");
            
            if self.is_initialized {
                // Simulate engine unload
                self.is_initialized = false;
                info!("Llama.cpp engine stopped (simulated)");
                
                // Update models status
                let mut models_vec = self.models.write().await;
                for model in models_vec.iter_mut() {
                    model.status = "unloaded".to_string();
                }
            }
            
            info!("Llama.cpp worker stopped successfully");
            Ok(())
        }
    }
}

impl Drop for LlamaEngine {
    fn drop(&mut self) {
        if self.is_initialized {
            info!("Cleaning up Llama.cpp engine on drop");
            // Simulate cleanup
            debug!("Llama.cpp engine cleaned up (simulated)");
        }
    }
}

// Additional utility functions for Llama.cpp engine
#[allow(dead_code)] // LlamaEngine utility methods
impl LlamaEngine {
    /// Get the current model status (enhanced with loading states)
    pub async fn get_model_status(&self) -> Result<String> {
        let status = self.loading_status.read().await;
        Ok(status.clone())
    }

    /// Load a new model dynamically
    pub async fn load_model(&mut self, model_path: &str) -> Result<()> {
        info!("Starting to load model: {}", model_path);
        
        // Set loading status
        {
            let mut status = self.loading_status.write().await;
            *status = "loading".to_string();
        }
        {
            let mut loading_model = self.current_loading_model.write().await;
            *loading_model = Some(model_path.to_string());
        }
        
        // Check if model file exists
        if !tokio::fs::metadata(model_path).await.is_ok() {
            let mut status = self.loading_status.write().await;
            *status = format!("error: Model file not found: {}", model_path);
            return Err(anyhow!("Model file not found: {}", model_path));
        }
        
        // Unload current model
        if self.is_initialized {
            info!("Unloading current model...");
            // Simulate unload
            self.is_initialized = false;
            debug!("Current model unloaded (simulated)");
        }
        
        // Load new model
        info!("Loading new model: {}", model_path);
        // Simulate engine initialization
        if std::path::Path::new(model_path).exists() {
            self.is_initialized = true;
            self.model_path = Some(model_path.to_string());
            
            // Update status to loaded
            {
                let mut status = self.loading_status.write().await;
                *status = "loaded".to_string();
            }
            
            info!("Model loaded successfully: {}", model_path);
            Ok(())
        } else {
            let mut status = self.loading_status.write().await;
            *status = format!("error: Model file not found: {}", model_path);
            Err(anyhow!("Model file not found: {}", model_path))
        }
    }

    /// Get current loaded model path
    pub async fn get_current_model(&self) -> String {
        self.model_path.clone().unwrap_or_default()
    }

    /// Check if model is loaded
    pub async fn is_model_loaded(&self) -> bool {
        let status = self.loading_status.read().await;
        status.as_str() == "loaded"
    }

    /// Get detailed loading status
    pub async fn get_loading_status(&self) -> String {
        let status = self.loading_status.read().await;
        let loading_model = self.current_loading_model.read().await;
        
        match status.as_str() {
            "loading" => {
                if let Some(model) = loading_model.as_ref() {
                    format!("Loading model: {}", model)
                } else {
                    "Loading...".to_string()
                }
            }
            "loaded" => {
                if let Some(model) = &self.model_path {
                    format!("Model loaded: {}", model)
                } else {
                    "Model loaded".to_string()
                }
            }
            "not_loaded" => "No model loaded".to_string(),
            other if other.starts_with("error:") => format!("Loading error: {}", &other[6..]),
            _ => format!("Unknown status: {}", status.as_str()),
        }
    }

    /// Generate text with custom parameters
    pub async fn generate_with_params(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        self.generate_response(prompt, max_tokens).await
    }

    /// Check if the engine is ready for inference
    pub async fn is_ready(&self) -> bool {
        self.is_initialized
    }

    /// Get engine configuration
    pub fn get_config(&self) -> Option<(String, u32, u32)> {
        self.model_path.as_ref().map(|path| (path.clone(), self.n_ctx, self.n_gpu_layers))
    }

    /// List available models in the models directory
    pub async fn list_local_models(&self) -> Result<Vec<String>> {
        let mut models = Vec::new();
        
        if !self.models_dir.exists() {
            return Ok(models);
        }

        let mut entries = fs::read_dir(&self.models_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "gguf" || ext == "bin" {
                        if let Some(filename) = path.file_name() {
                            models.push(filename.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        Ok(models)
    }

    /// Generate text using the loaded model (embedded mode)
    pub async fn generate(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        if !self.is_initialized {
            return Err(anyhow!("Llama.cpp engine is not initialized"));
        }

        debug!("Generating text with prompt: {}, max_tokens: {}", prompt, max_tokens);
        // Simulate text generation
        Ok(format!("Generated response for: {} (simulated, {} tokens)", &prompt[..prompt.len().min(30)], max_tokens))
    }

    /// Download a model from a URL
    pub async fn download_model(&self, url: &str, filename: &str) -> Result<PathBuf> {
        use reqwest::Client;
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        
        info!("Downloading model from {} to {}", url, filename);

        // Ensure models directory exists
        if !self.models_dir.exists() {
            fs::create_dir_all(&self.models_dir).await?;
        }

        let target_path = self.models_dir.join(filename);
        
        // Download the file
        let client = Client::new();
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to download model: HTTP {}", response.status()));
        }

        let total_size = response.content_length();
        let mut downloaded: u64 = 0;
        let mut file = fs::File::create(&target_path).await?;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;

            if let Some(total) = total_size {
                let percentage = (downloaded as f64 / total as f64) * 100.0;
                debug!("Download progress: {:.2}%", percentage);
            }
        }

        file.flush().await?;
        info!("Model downloaded successfully to {:?}", target_path);

        Ok(target_path)
    }

    /// Delete a model file
    pub async fn delete_model(&self, filename: &str) -> Result<()> {
        let model_path = self.models_dir.join(filename);
        
        if !model_path.exists() {
            return Err(anyhow!("Model file does not exist: {}", filename));
        }

        fs::remove_file(&model_path).await?;
        info!("Model deleted: {}", filename);

        Ok(())
    }

    /// Get model file size
    pub async fn get_model_size(&self, filename: &str) -> Result<u64> {
        let model_path = self.models_dir.join(filename);
        let metadata = fs::metadata(&model_path).await?;
        Ok(metadata.len())
    }

}
