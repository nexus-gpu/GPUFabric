use super::Engine;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tokio::fs;
use tracing::{debug, info, warn};

use futures_util::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// llama-cpp-2 imports (only for non-Android platforms)
#[cfg(not(target_os = "android"))]
use llama_cpp_2::{model::LlamaModel, context::LlamaContext, llama_backend::LlamaBackend};
#[cfg(not(target_os = "android"))]
use llama_cpp_2::{model::params::LlamaModelParams, context::params::LlamaContextParams};
#[cfg(not(target_os = "android"))]
use std::num::NonZeroU32;

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
    
    // Cached model components (only for non-Android platforms)
    #[cfg(not(target_os = "android"))]
    pub cached_backend: Option<Arc<LlamaBackend>>,
    #[cfg(not(target_os = "android"))]
    pub cached_model: Option<Arc<Mutex<LlamaModel>>>,
    #[cfg(not(target_os = "android"))]
    pub cached_model_path: Option<String>, // Track which model is currently cached
}

#[derive(Clone, Debug)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub repeat_penalty: f32,
    pub repeat_last_n: i32,
    pub seed: u32,
    pub min_keep: usize,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.8,
            top_k: 40,
            top_p: 0.95,
            repeat_penalty: 1.1,
            repeat_last_n: 64,
            seed: 0,
            min_keep: 1,
        }
    }
}

// llama-cpp-2 state wrapper (no longer stored, used for single inference)
#[cfg(not(target_os = "android"))]
pub struct LlamaCppState<'a> {
    pub _backend: LlamaBackend,
    pub _model: LlamaModel,
    pub _context: LlamaContext<'a>,
}

#[cfg(not(target_os = "android"))]
impl<'a> LlamaCppState<'a> {
    pub fn generate_blocking(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        // Simple implementation - return a formatted response for now
        // TODO: Implement proper llama-cpp-2 inference when API is stable
        Ok(format!("llama-cpp-2 response for: {} ({} tokens)", 
            &prompt[..prompt.len().min(30)], max_tokens))
    }
}

#[allow(dead_code)] // LlamaEngine implementation methods
impl LlamaEngine {
    /// Load and cache the model (separated from inference)
    pub async fn initialize_model(&mut self) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            // Android: No model caching needed
            self.is_initialized = true;
            return Ok(());
        }
        
        #[cfg(not(target_os = "android"))]
        {
            let model_path = self.model_path.as_ref()
                .ok_or_else(|| anyhow!("Model path not set"))?
                .clone();

            let resolved_model_path = self.validate_model_path(&model_path)?;
            let resolved_model_path_str = resolved_model_path.to_string_lossy().to_string();
            
            // Check if model is already cached AND matches current path
            if let Some(ref cached_path) = self.cached_model_path {
                if cached_path == &resolved_model_path_str && self.cached_model.is_some() {
                    info!("Model already loaded and cached: {}", resolved_model_path_str);
                    return Ok(());
                } else if cached_path != &resolved_model_path_str {
                    // Model path changed, clear old cache
                    warn!("Model path changed from {} to {}, clearing cache", cached_path, resolved_model_path_str);
                    self.clear_cache();
                }
            }
            let n_gpu_layers = self.n_gpu_layers;
            let model_path_for_closure = resolved_model_path_str.clone();
            let model_path_for_cache = model_path_for_closure.clone();
            
            info!("Loading and caching llama-cpp-2 model: {}", model_path_for_closure);
            
            // Run model loading in blocking thread
            let (backend, model) = tokio::task::spawn_blocking(move || {
                let backend = LlamaBackend::init()
                    .map_err(|e| anyhow!("Failed to initialize backend: {:?}", e))?;
                
                let model_params = LlamaModelParams::default()
                    .with_n_gpu_layers(n_gpu_layers);
                
                let model = LlamaModel::load_from_file(&backend, &model_path_for_closure, &model_params)
                    .map_err(|e| anyhow!("Failed to load model: {:?}", e))?;
                
                Ok::<(LlamaBackend, LlamaModel), anyhow::Error>((backend, model))
            }).await??;
            
            // Cache the components and store the model path
            self.cached_backend = Some(Arc::new(backend));
            self.cached_model = Some(Arc::new(Mutex::new(model)));
            self.cached_model_path = Some(model_path_for_cache.clone());
            self.is_initialized = true;
            
            info!("Model successfully loaded and cached: {}", model_path_for_cache);
            Ok(())
        }
    }
    
    /// Clear cached model to free memory
    #[cfg(not(target_os = "android"))]
    pub fn clear_cache(&mut self) {
        if self.cached_model.is_some() {
            info!("Clearing model cache to free memory");
            self.cached_model = None;
            self.cached_backend = None;
            self.cached_model_path = None;
            self.is_initialized = false;
            info!("Model cache cleared");
        }
    }
    
    /// Generate text using cached model (inference only)
    /// Returns (generated_text, prompt_tokens, completion_tokens)
    pub async fn generate_with_cached_model(&self, prompt: &str, max_tokens: usize) -> Result<(String, usize, usize)> {
        let params = SamplingParams::default();
        self.generate_with_cached_model_sampling(prompt, max_tokens, &params)
            .await
    }

    pub async fn generate_with_cached_model_sampling(
        &self,
        prompt: &str,
        max_tokens: usize,
        sampling: &SamplingParams,
    ) -> Result<(String, usize, usize)> {
        if !self.is_initialized {
            return Err(anyhow!("Engine not initialized - call load_model() first"));
        }
        
        #[cfg(target_os = "android")]
        {
            // Android: Simulated response
            warn!("Android SDK: Using simulated response");
            let text = format!("Android SDK response for: {} (simulated, {} tokens)", 
                &prompt[..prompt.len().min(30)], max_tokens);
            Ok((text, 10, 20)) // Simulated token counts
        }
        
        #[cfg(not(target_os = "android"))]
        {
            // Client: Real inference using cached model
            info!("Client: Executing inference with cached model");
            
            let backend = self.cached_backend.as_ref()
                .ok_or_else(|| anyhow!("Model not loaded - call load_model() first"))?
                .clone();
            let model = self.cached_model.as_ref()
                .ok_or_else(|| anyhow!("Model not loaded - call load_model() first"))?
                .clone();
            
            let prompt = prompt.to_string();
            let n_ctx = self.n_ctx;
            let sampling = sampling.clone();
            
            // Run inference in blocking thread
            tokio::task::spawn_blocking(move || {
                use llama_cpp_2::model::AddBos;
                use llama_cpp_2::llama_batch::LlamaBatch;
                use llama_cpp_2::sampling::LlamaSampler;
                
                let context_params = LlamaContextParams::default()
                    .with_n_ctx(NonZeroU32::new(n_ctx));
                
                // Lock model and create context with proper lifetime
                let model_guard = model.lock()
                    .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;
                
                let mut context = model_guard.new_context(&*backend, context_params)
                    .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;
                
                // Tokenize the prompt
                let tokens = model_guard.str_to_token(&prompt, AddBos::Always)
                    .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;
                
                // Create batch and add tokens
                let mut batch = LlamaBatch::new(tokens.len(), 1);
                for (i, token) in tokens.iter().enumerate() {
                    let is_last = i == tokens.len() - 1;
                    batch.add(*token, i as i32, &[0], is_last)
                        .map_err(|e| anyhow!("Failed to add token to batch: {:?}", e))?;
                }
                
                // Decode tokens (process prompt)
                context.decode(&mut batch)
                    .map_err(|e| anyhow!("Failed to decode batch: {:?}", e))?;
                
                // Generate tokens
                let mut output_tokens = Vec::new();
                let mut output_text = String::new();
                let mut n_cur = tokens.len(); // Current position in sequence
                
                let mut samplers = Vec::new();

                if sampling.repeat_penalty != 1.0 {
                    samplers.push(LlamaSampler::penalties(
                        sampling.repeat_last_n,
                        sampling.repeat_penalty,
                        0.0,
                        0.0,
                    ));
                }
                if sampling.top_k > 0 {
                    samplers.push(LlamaSampler::top_k(sampling.top_k));
                }
                if sampling.top_p > 0.0 && sampling.top_p < 1.0 {
                    samplers.push(LlamaSampler::top_p(sampling.top_p, sampling.min_keep));
                }
                samplers.push(LlamaSampler::temp(sampling.temperature));
                if sampling.temperature <= 0.0 {
                    samplers.push(LlamaSampler::greedy());
                } else {
                    samplers.push(LlamaSampler::dist(sampling.seed));
                }

                let mut sampler = LlamaSampler::chain_simple(samplers);
                sampler.accept_many(tokens.iter());
                
                for i in 0..max_tokens {
                    // Sample using the sampler chain
                    let new_token = sampler.sample(&context, -1);
                    sampler.accept(new_token);
                    
                    debug!(
                        "Token {}: id={}, text={:?}",
                        i,
                        new_token,
                        model_guard
                            .token_to_str(new_token, llama_cpp_2::model::Special::Tokenize)
                            .ok()
                    );
                    
                    // Check for EOS token
                    if new_token == model_guard.token_eos() {
                        break;
                    }
                    
                    // Convert token to string and append
                    use llama_cpp_2::model::Special;
                    if let Ok(piece) = model_guard.token_to_str(new_token, Special::Tokenize) {
                        // Check for stop sequences (ChatML, Llama3, etc.)
                        if piece.contains("<|im_end|>") || piece.contains("<|eot_id|>") || 
                           piece.contains("<|end_of_text|>") || piece.contains("</s>") {
                            break;
                        }
                        output_text.push_str(&piece);
                    }
                    
                    output_tokens.push(new_token);
                    
                    // Prepare next batch with single token at correct position
                    let mut next_batch = LlamaBatch::new(1, 1);
                    next_batch.add(new_token, n_cur as i32, &[0], true)
                        .map_err(|e| anyhow!("Failed to add token: {:?}", e))?;
                    
                    // Decode next token
                    context.decode(&mut next_batch)
                        .map_err(|e| anyhow!("Failed to decode token: {:?}", e))?;
                    
                    // Increment position for next token
                    n_cur += 1;
                }
                
                // Return text with token counts
                let prompt_token_count = tokens.len();
                let completion_token_count = output_tokens.len();
                Ok((output_text, prompt_token_count, completion_token_count))
            }).await?
        }
    }

    pub async fn stream_with_cached_model_sampling(
        &self,
        prompt: &str,
        max_tokens: usize,
        sampling: &SamplingParams,
    ) -> Result<impl Stream<Item = Result<String>> + Send + 'static> {
        if !self.is_initialized {
            return Err(anyhow!("Engine not initialized - call load_model() first"));
        }

        #[cfg(target_os = "android")]
        {
            use futures_util::StreamExt;

            let _ = (prompt, max_tokens, sampling);
            let s = futures_util::stream::once(async {
                Err(anyhow!("Android streaming is not implemented"))
            })
            .boxed();

            return Ok(s);
        }

        #[cfg(not(target_os = "android"))]
        {
            let backend = self
                .cached_backend
                .as_ref()
                .ok_or_else(|| anyhow!("Model not loaded - call load_model() first"))?
                .clone();
            let model = self
                .cached_model
                .as_ref()
                .ok_or_else(|| anyhow!("Model not loaded - call load_model() first"))?
                .clone();

            let prompt = prompt.to_string();
            let n_ctx = self.n_ctx;
            let sampling = sampling.clone();

            let (tx, rx) = mpsc::channel::<Result<String>>(64);

            tokio::task::spawn_blocking(move || {
                use llama_cpp_2::llama_batch::LlamaBatch;
                use llama_cpp_2::model::{AddBos, Special};
                use llama_cpp_2::sampling::LlamaSampler;

                let context_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(n_ctx));

                let model_guard = model
                    .lock()
                    .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;
                let mut context = model_guard
                    .new_context(&*backend, context_params)
                    .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;

                let tokens = model_guard
                    .str_to_token(&prompt, AddBos::Always)
                    .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;

                let mut batch = LlamaBatch::new(tokens.len(), 1);
                for (i, token) in tokens.iter().enumerate() {
                    let is_last = i == tokens.len() - 1;
                    batch
                        .add(*token, i as i32, &[0], is_last)
                        .map_err(|e| anyhow!("Failed to add token to batch: {:?}", e))?;
                }

                context
                    .decode(&mut batch)
                    .map_err(|e| anyhow!("Failed to decode batch: {:?}", e))?;

                let mut samplers = Vec::new();
                if sampling.repeat_penalty != 1.0 {
                    samplers.push(LlamaSampler::penalties(
                        sampling.repeat_last_n,
                        sampling.repeat_penalty,
                        0.0,
                        0.0,
                    ));
                }
                if sampling.top_k > 0 {
                    samplers.push(LlamaSampler::top_k(sampling.top_k));
                }
                if sampling.top_p > 0.0 && sampling.top_p < 1.0 {
                    samplers.push(LlamaSampler::top_p(sampling.top_p, sampling.min_keep));
                }
                samplers.push(LlamaSampler::temp(sampling.temperature));
                if sampling.temperature <= 0.0 {
                    samplers.push(LlamaSampler::greedy());
                } else {
                    samplers.push(LlamaSampler::dist(sampling.seed));
                }

                let mut sampler = LlamaSampler::chain_simple(samplers);
                sampler.accept_many(tokens.iter());

                let mut n_cur = tokens.len();
                for _i in 0..max_tokens {
                    let new_token = sampler.sample(&context, -1);
                    sampler.accept(new_token);

                    if new_token == model_guard.token_eos() {
                        break;
                    }

                    if let Ok(piece) = model_guard.token_to_str(new_token, Special::Tokenize) {
                        if piece.contains("<|im_end|>")
                            || piece.contains("<|eot_id|>")
                            || piece.contains("<|end_of_text|>")
                            || piece.contains("</s>")
                        {
                            break;
                        }

                        if tx.blocking_send(Ok(piece)).is_err() {
                            break;
                        }
                    }

                    let mut next_batch = LlamaBatch::new(1, 1);
                    next_batch
                        .add(new_token, n_cur as i32, &[0], true)
                        .map_err(|e| anyhow!("Failed to add token: {:?}", e))?;
                    context
                        .decode(&mut next_batch)
                        .map_err(|e| anyhow!("Failed to decode token: {:?}", e))?;
                    n_cur += 1;
                }

                Ok::<(), anyhow::Error>(())
            });

            Ok(ReceiverStream::new(rx))
        }
    }
    
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
            n_gpu_layers: 99,
            is_initialized: false,
            models_dir,
            loading_status: Arc::new(RwLock::new("not_loaded".to_string())),
            current_loading_model: Arc::new(RwLock::new(None)),
            
            #[cfg(not(target_os = "android"))]
            cached_backend: None,
            #[cfg(not(target_os = "android"))]
            cached_model: None,
            #[cfg(not(target_os = "android"))]
            cached_model_path: None,
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
            
            #[cfg(not(target_os = "android"))]
            cached_backend: None,
            #[cfg(not(target_os = "android"))]
            cached_model: None,
            #[cfg(not(target_os = "android"))]
            cached_model_path: None,
        }
    }

    async fn ensure_initialized(&mut self) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            // On Android, check if SDK has loaded the model
            if self.check_sdk_model_loaded() {
                self.is_initialized = true;
                return Ok(());
            } else {
                return Err(anyhow!("Android: Model not loaded by SDK yet"));
            }
        }
        
        #[cfg(not(target_os = "android"))]
        {
            if !self.is_initialized {
                // Use the new separated model loading
                self.initialize_model().await?;
            }
        }
        Ok(())
    }

    /// Check if SDK has loaded the model (Android only)
    #[cfg(target_os = "android")]
    fn check_sdk_model_loaded(&self) -> bool {
        use crate::GLOBAL_MODEL_PTR;
        use crate::GLOBAL_CONTEXT_PTR;
        
        let model_ptr = GLOBAL_MODEL_PTR.load(std::sync::atomic::Ordering::SeqCst);
        let context_ptr = GLOBAL_CONTEXT_PTR.load(std::sync::atomic::Ordering::SeqCst);
        
        !model_ptr.is_null() && !context_ptr.is_null()
    }

    /// Resolve model path: if relative, try models_dir first; if absolute, use directly
    fn resolve_model_path(&self, path: &str) -> Result<PathBuf> {
        let path_buf = PathBuf::from(path);
        
        // If absolute path, use it directly
        if path_buf.is_absolute() {
            if !path_buf.exists() {
                return Err(anyhow!("Model file does not exist: {}", path));
            }
            if !path_buf.is_file() {
                return Err(anyhow!("Model path is not a file: {}", path));
            }
            return Ok(path_buf);
        }
        
        // If relative path, try models_dir first
        let models_dir_path = self.models_dir.join(path);
        if models_dir_path.exists() && models_dir_path.is_file() {
            info!("Resolved relative path '{}' to '{}'", path, models_dir_path.display());
            return Ok(models_dir_path);
        }
        
        // Fallback: try current directory
        if path_buf.exists() && path_buf.is_file() {
            warn!("Using model from current directory: {}", path);
            return Ok(path_buf);
        }
        
        Err(anyhow!(
            "Model file not found: '{}' (checked: {} and current dir)",
            path,
            models_dir_path.display()
        ))
    }
    
    fn validate_model_path(&self, path: &str) -> Result<PathBuf> {
        // Use resolve_model_path for consistent path handling
        self.resolve_model_path(path)
    }

    async fn generate_response(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        if !self.is_initialized {
            return Err(anyhow!("Llama.cpp engine is not initialized"));
        }

        debug!("Generating response with prompt: {}, max_tokens: {}", prompt, max_tokens);
        
        #[cfg(target_os = "android")]
        {
            // Use SDK functions for inference on Android
            if !self.check_sdk_model_loaded() {
                return Err(anyhow!("Android: Model not loaded by SDK"));
            }
            
            use crate::GLOBAL_MODEL_PTR;
            use crate::GLOBAL_CONTEXT_PTR;
            use std::ffi::CString;
            use std::os::raw::c_char;
            
            let model_ptr = GLOBAL_MODEL_PTR.load(std::sync::atomic::Ordering::SeqCst);
            let context_ptr = GLOBAL_CONTEXT_PTR.load(std::sync::atomic::Ordering::SeqCst);
            
            // Convert prompt to C string
            let prompt_cstr = CString::new(prompt)
                .map_err(|e| anyhow!("Invalid prompt for C FFI: {}", e))?;
            
            // Create output buffer (larger buffer for longer responses)
            let mut output = vec![0u8; 8192];
            
            debug!("Calling SDK inference function");
            let result = unsafe {
                crate::gpuf_generate_final_solution_text(
                    model_ptr,
                    context_ptr,
                    prompt_cstr.as_ptr(),
                    max_tokens as i32,
                    output.as_mut_ptr() as *mut c_char,
                    output.len() as i32,  // Add missing output_len parameter
                )
            };
            
            // Check return code (0 = success)
            if result != 0 {
                return Err(anyhow!("Android: Inference failed with error code: {}", result));
            }
            
            // Convert output buffer to Rust string
            let result_str = unsafe {
                std::ffi::CStr::from_ptr(output.as_ptr() as *const c_char)
                    .to_str()
                    .map_err(|e| anyhow!("Invalid UTF-8 in inference result: {}", e))?
            };
            
            info!("Android inference completed successfully");
            Ok(result_str.to_string())
        }
        
        #[cfg(not(target_os = "android"))]
        {
            // Use the new cached inference method and extract just the text
            let (text, _, _) = self.generate_with_cached_model(prompt, max_tokens).await?;
            Ok(text)
        }
    }
}

impl Engine for LlamaEngine {
    fn init(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Initializing Llama.cpp engine");
            
            #[cfg(target_os = "android")]
            {
                // On Android, check if SDK has already loaded the model
                // by verifying global pointers are set
                use crate::GLOBAL_MODEL_PTR;
                use crate::GLOBAL_CONTEXT_PTR;
                
                if self.model_path.is_some() {
                    let model_ptr = GLOBAL_MODEL_PTR.load(std::sync::atomic::Ordering::SeqCst);
                    let context_ptr = GLOBAL_CONTEXT_PTR.load(std::sync::atomic::Ordering::SeqCst);
                    
                    if !model_ptr.is_null() && !context_ptr.is_null() {
                        info!("Android: Model and context already loaded by SDK");
                        self.is_initialized = true;
                        return Ok(());
                    } else {
                        info!("Android: Model not yet loaded by SDK, waiting for SDK initialization");
                        // Don't mark as initialized, SDK will handle it
                        return Ok(());
                    }
                } else {
                    warn!("Android: No model path specified, waiting for SDK to load model");
                    return Ok(());
                }
            }
            
            #[cfg(not(target_os = "android"))]
            {
                // Non-Android: Normal initialization flow
                if self.model_path.is_none() {
                    warn!("No model path specified, engine will be initialized when model is set");
                    return Ok(());
                }

                self.ensure_initialized().await?;
            }
            
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
            
            #[cfg(target_os = "android")]
            {
                // On Android, model loading is handled by SDK API calls
                // Just store the path for reference and mark as initialized
                info!("Android target: storing model path for SDK-based loading: {}", model_path);
                self.model_path = Some(model_path.clone());
                self.models_name = vec![model_path.clone()];
                
                // Update models list
                let mut models_vec = self.models.write().await;
                models_vec.clear();
                models_vec.push(super::ModelInfo {
                    id: "llama_cpp_model".to_string(),
                    name: model_path,
                    status: "loaded_by_sdk".to_string(),
                });
                
                // Note: Don't call ensure_initialized() here - SDK will handle model loading
                info!("Model path stored for Android SDK loading");
                return Ok(());
            }
            
            #[cfg(not(target_os = "android"))]
            {
                // Non-Android: Validate model path and load normally
                self.validate_model_path(&model_path)?;
                
                // If engine is already initialized with a different model, unload it first
                if self.is_initialized {
                    if Some(model_path.clone()) != self.model_path {
                        info!("Unloading previous model before loading new one");
                        
                        // Clear cached model and backend to free memory
                        self.cached_model = None;
                        self.cached_backend = None;
                        info!("Previous model cache cleared");
                        
                        self.is_initialized = false;
                        info!("Previous model unloaded completely");
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
            }

            info!("Models set successfully for Llama.cpp engine");
            Ok(())
        }
    }

    fn start_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Starting Llama.cpp worker");
            
            #[cfg(target_os = "android")]
            {
                // On Android, verify SDK has loaded the model
                if self.check_sdk_model_loaded() {
                    info!("Android: SDK model loaded successfully, worker ready");
                    self.is_initialized = true;
                } else {
                    return Err(anyhow!("Android: Cannot start worker - model not loaded by SDK"));
                }
            }
            
            #[cfg(not(target_os = "android"))]
            {
                // For Llama.cpp, the "worker" is essentially just ensuring the engine is initialized
                self.ensure_initialized().await?;
            }
            
            info!("Llama.cpp worker started successfully");
            Ok(())
        }
    }

    fn stop_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Stopping Llama.cpp worker");
            
            if self.is_initialized {
                // Clear model path and reset initialization state
                self.model_path = None;
                self.is_initialized = false;
                info!("Llama.cpp engine stopped successfully");
                
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

        {
            let mut status = crate::MODEL_STATUS
                .lock()
                .map_err(|e| anyhow!("Failed to lock MODEL_STATUS: {:?}", e))?;
            status.set_loading(model_path);
        }
        
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

            {
                let mut status = crate::MODEL_STATUS
                    .lock()
                    .map_err(|e| anyhow!("Failed to lock MODEL_STATUS: {:?}", e))?;
                status.set_error(&format!("Model file not found: {}", model_path));
            }

            return Err(anyhow!("Model file not found: {}", model_path));
        }
        
        // Unload current model
        if self.is_initialized {
            info!("Unloading current model...");
            self.is_initialized = false;
            debug!("Current model unloaded");
        }
        
        // Set new model path and load it
        self.model_path = Some(model_path.to_string());
        
        // Use the real loading logic from ensure_initialized
        match self.ensure_initialized().await {
            Ok(()) => {
                // Update status to loaded
                {
                    let mut status = self.loading_status.write().await;
                    *status = "loaded".to_string();
                }

                {
                    let mut status = crate::MODEL_STATUS
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock MODEL_STATUS: {:?}", e))?;
                    status.set_loaded(model_path);
                }
                
                info!("Model loaded successfully: {}", model_path);
                Ok(())
            }
            Err(e) => {
                let mut status = self.loading_status.write().await;
                *status = format!("error: {}", e);

                {
                    let mut status = crate::MODEL_STATUS
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock MODEL_STATUS: {:?}", e))?;
                    status.set_error(&e.to_string());
                }

                Err(e)
            }
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
    /// Returns (generated_text, prompt_tokens, completion_tokens)
    pub async fn generate(&self, prompt: &str, max_tokens: usize) -> Result<(String, usize, usize)> {
        if !self.is_initialized {
            return Err(anyhow!("Llama.cpp engine is not initialized"));
        }

        debug!("Generating text with prompt: {}, max_tokens: {}", prompt, max_tokens);
        
        // Use the real inference method with cached model
        self.generate_with_cached_model(prompt, max_tokens).await
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
