use anyhow::{Result, anyhow};
use std::path::Path;

/// Temporary stub implementation so Android build can succeed
pub struct LlamaEngine;

pub fn init_global_engine(
    _model_path: impl AsRef<Path>,
    _n_ctx: u32,
    _n_gpu_layers: u32,
) -> Result<()> {
    Err(anyhow!("llama backend is disabled for this build"))
}

pub fn generate_text(_prompt: &str, _max_tokens: usize) -> Result<String> {
    Err(anyhow!("llama backend is disabled for this build"))
}
