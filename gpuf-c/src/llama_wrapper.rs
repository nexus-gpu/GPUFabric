use anyhow::{Result, anyhow};
use std::path::Path;
use std::sync::Mutex;
#[cfg(not(target_os = "android"))]
use once_cell::sync::Lazy;
#[cfg(target_os = "android")]
use std::sync::OnceLock;
use tracing::{info, debug};

#[cfg(not(target_os = "android"))]
use llama_cpp_2::{
    model::LlamaModel,
    llama_backend::LlamaBackend,
    model::{AddBos, Special},
    model::params::LlamaModelParams,
    context::params::LlamaContextParams,
    llama_batch::LlamaBatch,
    sampling::LlamaSampler,
};

// Global model instance for singleton pattern
#[cfg(not(target_os = "android"))]
static GLOBAL_LLAMA: Lazy<Mutex<Option<LlamaInstance>>> = Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "android")]
static GLOBAL_LLAMA: OnceLock<Mutex<Option<LlamaInstance>>> = OnceLock::new();

#[cfg(not(target_os = "android"))]
struct LlamaInstance {
    _backend: LlamaBackend,
    model: LlamaModel,
}

#[cfg(target_os = "android")]
struct LlamaInstance;

/// Llama.cpp engine
#[allow(dead_code)] // Llama.cpp engine wrapper structure
pub struct LlamaEngine;

#[cfg(not(target_os = "android"))]
impl LlamaInstance {
    fn new(model_path: &Path, _n_ctx: u32, n_gpu_layers: u32) -> Result<Self> {
        info!("Initializing LlamaModel from: {:?}", model_path);
        
        // Initialize backend
        let backend = LlamaBackend::init().map_err(|e| anyhow!("Failed to initialize backend: {}", e))?;
        
        let model_params = LlamaModelParams::default()
            .with_n_gpu_layers(n_gpu_layers);
        
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| anyhow!("Failed to load model: {}", e))?;
        
        info!("Model loaded successfully");
        
        Ok(LlamaInstance { _backend: backend, model })
    }
    
    fn generate_text(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        debug!("Generating text with prompt: '{}', max_tokens: {}", prompt, max_tokens);
        
        // Convert prompt to tokens
        let tokens = self.model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| anyhow!("Failed to tokenize prompt: {}", e))?;
        
        debug!("Tokenized prompt into {} tokens", tokens.len());
        
        // Create context parameters
        let context_params = LlamaContextParams::default()
            .with_n_ctx(Some(std::num::NonZeroU32::new(2048).unwrap()));
        
        // Create context
        let mut context = self.model.new_context(&self._backend, context_params)
            .map_err(|e| anyhow!("Failed to create context: {}", e))?;
        
        // Create batch
        let mut batch = LlamaBatch::new(2048, 1);
        
        // Clear batch
        batch.clear();
        
        // Add prompt tokens to batch
        let last_index = tokens.len() - 1;
        for (i, &token) in tokens.iter().enumerate() {
            // Only the last token needs logits for generation
            let needs_logits = i == last_index;
            batch.add(token, i as i32, &[0], needs_logits)?;
        }
        
        // Decode batch
        context.decode(&mut batch)
            .map_err(|e| anyhow!("Failed to decode prompt batch: {}", e))?;
        
        // Create sampler chain
        // Note: The chain must end with a final sampler like dist() or greedy()
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.7f32),
            LlamaSampler::top_k(40i32),
            LlamaSampler::top_p(0.95f32, 1usize),
            LlamaSampler::dist(1234u32),  // Final sampler - required!
        ]);
        
        // Start generation
        let mut generated_tokens = Vec::new();
        let mut remaining_tokens = max_tokens;
        let mut pos = tokens.len() as i32;
        
        while remaining_tokens > 0 {
            // Sample token from the last token in batch
            let token = sampler.sample(&context, batch.n_tokens() - 1);
            
            // Accept the token
            sampler.accept(token);
            
            // Check if it's end token
            if token == self.model.token_eos() {
                debug!("Encountered EOS token, stopping generation");
                break;
            }
            
            generated_tokens.push(token);
            
            // Clear batch
            batch.clear();
            
            // Add generated token to batch (needs logits for next generation)
            batch.add(token, pos, &[0], true)?;
            
            pos += 1;
            remaining_tokens -= 1;
            
            // Decode
            context.decode(&mut batch)
                .map_err(|e| anyhow!("Failed to decode generated token: {}", e))?;
        }
        
        // Convert generated tokens back to text
        let generated_text = self.model
            .tokens_to_str(&generated_tokens, Special::Tokenize)
            .map_err(|e| anyhow!("Failed to detokenize: {}", e))?;
        
        debug!("Generated {} characters", generated_text.len());
        Ok(generated_text)
    }
}

#[cfg(target_os = "android")]
impl LlamaInstance {
    fn new(_model_path: &Path, _n_ctx: u32, _n_gpu_layers: u32) -> Result<Self> {
        Err(anyhow!("Llama.cpp is not supported on Android due to POSIX compatibility issues. Consider using ONNX or remote inference instead."))
    }
    
    fn generate_text(&mut self, _prompt: &str, _max_tokens: usize) -> Result<String> {
        Err(anyhow!("Llama.cpp is not supported on Android due to POSIX compatibility issues. Consider using ONNX or remote inference instead."))
    }
}

/// Initialize global LLM engine
pub fn init_global_engine(
    model_path: impl AsRef<Path>,
    n_ctx: u32,
    n_gpu_layers: u32,
) -> Result<()> {
    let path = model_path.as_ref();
    
    if !path.exists() {
        return Err(anyhow!("Model file does not exist: {:?}", path));
    }
    
    info!("Initializing global LLM engine with model: {:?}", path);
    info!("Context size: {}, GPU layers: {}", n_ctx, n_gpu_layers);
    
    let instance = LlamaInstance::new(path, n_ctx, n_gpu_layers)?;
    
    #[cfg(not(target_os = "android"))]
    {
        let mut global = GLOBAL_LLAMA.lock().map_err(|e| anyhow!("Failed to acquire lock: {}", e))?;
        *global = Some(instance);
    }
    
    #[cfg(target_os = "android")]
    {
        let global = GLOBAL_LLAMA.get_or_init(|| Mutex::new(None));
        let mut guard = global.lock().map_err(|e| anyhow!("Failed to acquire lock: {}", e))?;
        *guard = Some(instance);
    }
    
    info!("Global LLM engine initialized successfully");
    Ok(())
}

/// Generate text
pub fn generate_text(prompt: &str, max_tokens: usize) -> Result<String> {
    #[cfg(not(target_os = "android"))]
    {
        let mut global = GLOBAL_LLAMA.lock().map_err(|e| anyhow!("Failed to acquire lock: {}", e))?;
        
        match global.as_mut() {
            Some(instance) => {
                info!("Generating text with {} max tokens", max_tokens);
                instance.generate_text(prompt, max_tokens)
            }
            None => Err(anyhow!("LLM engine not initialized. Call init_global_engine first."))
        }
    }
    
    #[cfg(target_os = "android")]
    {
        let global = GLOBAL_LLAMA.get().ok_or_else(|| anyhow!("LLM engine not initialized. Call init_global_engine first."))?;
        let mut guard = global.lock().map_err(|e| anyhow!("Failed to acquire lock: {}", e))?;
        
        match guard.as_mut() {
            Some(instance) => {
                info!("Generating text with {} max tokens", max_tokens);
                instance.generate_text(prompt, max_tokens)
            }
            None => Err(anyhow!("LLM engine not initialized. Call init_global_engine first."))
        }
    }
}

/// Check if engine is initialized
pub fn is_initialized() -> bool {
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(global) = GLOBAL_LLAMA.lock() {
            global.is_some()
        } else {
            false
        }
    }
    
    #[cfg(target_os = "android")]
    {
        if let Some(global) = GLOBAL_LLAMA.get() {
            if let Ok(guard) = global.lock() {
                guard.is_some()
            } else {
                false
            }
        } else {
            false
        }
    }
}

/// Unload global engine
pub fn unload_global_engine() -> Result<()> {
    info!("Unloading global LLM engine");
    
    #[cfg(not(target_os = "android"))]
    {
        let mut global = GLOBAL_LLAMA.lock().map_err(|e| anyhow!("Failed to acquire lock: {}", e))?;
        *global = None;
    }
    
    #[cfg(target_os = "android")]
    {
        if let Some(global) = GLOBAL_LLAMA.get() {
            let mut guard = global.lock().map_err(|e| anyhow!("Failed to acquire lock: {}", e))?;
            *guard = None;
        }
    }
    
    info!("Global LLM engine unloaded");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_is_initialized() {
        // Initial state should be uninitialized
        assert!(!is_initialized());
    }
    
    #[test]
    fn test_unload_without_init() {
        // Unload should succeed when not initialized
        assert!(unload_global_engine().is_ok());
    }
    
    #[test]
    fn test_generate_without_init() {
        // Generate text should fail when not initialized
        let result = generate_text("Hello", 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not initialized"));
    }
}
