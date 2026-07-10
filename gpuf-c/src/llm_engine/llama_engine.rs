use super::Engine;
use anyhow::{anyhow, Result};
#[cfg(not(target_os = "android"))]
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "android"))]
use std::fs as std_fs;
#[cfg(not(target_os = "android"))]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(target_os = "android"))]
use std::sync::Mutex;
#[cfg(not(target_os = "android"))]
use std::time::SystemTime;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use futures_util::Stream;
#[cfg(not(target_os = "android"))]
use sha2::{Digest, Sha256};
#[cfg(not(target_os = "android"))]
use tokio::sync::mpsc;
#[cfg(not(target_os = "android"))]
use tokio_stream::wrappers::ReceiverStream;

#[cfg(not(target_os = "android"))]
use crate::handle::session_cache::{
    record_worker_state_checkpoint_error, record_worker_state_checkpoint_hit,
    record_worker_state_checkpoint_miss, record_worker_state_checkpoint_quota_eviction,
    record_worker_state_checkpoint_reset, record_worker_state_checkpoint_save,
    set_worker_state_checkpoint_bytes_current, set_worker_state_checkpoint_max_bytes,
};
use crate::util::cmd::LlamaSplitModeArg;

// llama-cpp-2 imports (only for non-Android platforms)
#[cfg(all(not(target_os = "android"), feature = "multimodal"))]
use llama_cpp_2::mtmd::{
    mtmd_default_marker, MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText,
};
#[cfg(not(target_os = "android"))]
use llama_cpp_2::{
    context::params::{LlamaAttentionType, LlamaContextParams, LlamaPoolingType},
    model::params::LlamaModelParams,
};
#[cfg(not(target_os = "android"))]
use llama_cpp_2::{context::LlamaContext, llama_backend::LlamaBackend, model::LlamaModel};
#[cfg(not(target_os = "android"))]
use std::num::NonZeroU32;
#[cfg(not(target_os = "android"))]
use std::sync::OnceLock;

// Global backend instance - initialized only once
#[cfg(not(target_os = "android"))]
static LLAMA_BACKEND: OnceLock<Arc<LlamaBackend>> = OnceLock::new();

#[cfg(not(target_os = "android"))]
const SESSION_STATE_CACHE_DIR_ENV: &str = "GPUF_WORKER_SESSION_STATE_DIR";
#[cfg(not(target_os = "android"))]
const SESSION_STATE_CACHE_ENABLE_ENV: &str = "GPUF_ENABLE_SESSION_STATE_CACHE";
#[cfg(not(target_os = "android"))]
const SESSION_STATE_CACHE_ENABLE_KV_ENV: &str = "GPUF_ENABLE_SESSION_KV_CACHE";
#[cfg(not(target_os = "android"))]
const SESSION_STATE_CACHE_MAX_BYTES_ENV: &str = "GPUF_WORKER_SESSION_STATE_MAX_BYTES";
#[cfg(not(target_os = "android"))]
const SESSION_STATE_META_VERSION: u32 = 1;

#[cfg(not(target_os = "android"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionStateCachePolicy {
    Auto,
    Bypass,
    Reset,
    Unknown,
}

#[cfg(not(target_os = "android"))]
impl SessionStateCachePolicy {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            None => Self::Auto,
            Some(value) if value.eq_ignore_ascii_case("auto") => Self::Auto,
            Some(value) if value.eq_ignore_ascii_case("bypass") => Self::Bypass,
            Some(value) if value.eq_ignore_ascii_case("reset") => Self::Reset,
            Some(_) => Self::Unknown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bypass => "bypass",
            Self::Reset => "reset",
            Self::Unknown => "unknown",
        }
    }

    fn should_load(self) -> bool {
        matches!(self, Self::Auto)
    }

    fn should_save(self) -> bool {
        matches!(self, Self::Auto | Self::Reset)
    }
}

#[cfg(not(target_os = "android"))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionStateCachePlan {
    policy: SessionStateCachePolicy,
    session_hash_short: String,
    session_dir: PathBuf,
    state_path: PathBuf,
    model_key_hash: String,
    prompt_hash: String,
}

#[cfg(not(target_os = "android"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SessionStateCheckpointMeta {
    version: u32,
    model_key_hash: String,
    prompt_hash: String,
    state_sha256: String,
}

#[cfg(not(target_os = "android"))]
fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "android"))]
fn session_state_cache_enabled() -> bool {
    env_flag_enabled(SESSION_STATE_CACHE_ENABLE_ENV)
        || env_flag_enabled(SESSION_STATE_CACHE_ENABLE_KV_ENV)
}

#[cfg(not(target_os = "android"))]
fn session_state_cache_max_bytes() -> u64 {
    std::env::var(SESSION_STATE_CACHE_MAX_BYTES_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(not(target_os = "android"))]
fn session_state_cache_root() -> PathBuf {
    if let Ok(path) = std::env::var(SESSION_STATE_CACHE_DIR_ENV) {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("gpufabric")
        .join("session-state")
}

#[cfg(not(target_os = "android"))]
fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        let len = part.len() as u64;
        hasher.update(len.to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(not(target_os = "android"))]
fn sha256_str_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(not(target_os = "android"))]
fn llama_split_mode_key(mode: &LlamaSplitModeArg) -> &'static str {
    match mode {
        LlamaSplitModeArg::None => "none",
        LlamaSplitModeArg::Layer => "layer",
        LlamaSplitModeArg::Row => "row",
    }
}

#[cfg(not(target_os = "android"))]
fn model_cache_key(
    model_path: Option<&str>,
    n_ctx: u32,
    n_batch: u32,
    n_gpu_layers: u32,
    llama_split_mode: &LlamaSplitModeArg,
    llama_main_gpu: i32,
    llama_devices: Option<&str>,
) -> String {
    let model_path = model_path.unwrap_or("unloaded");
    let devices = llama_devices.unwrap_or("");
    let n_ctx = n_ctx.to_string();
    let n_batch = n_batch.to_string();
    let n_gpu_layers = n_gpu_layers.to_string();
    let llama_main_gpu = llama_main_gpu.to_string();
    sha256_hex(&[
        "model-cache-v1",
        model_path,
        n_ctx.as_str(),
        n_batch.as_str(),
        n_gpu_layers.as_str(),
        llama_split_mode_key(llama_split_mode),
        llama_main_gpu.as_str(),
        devices,
    ])
}

#[cfg(not(target_os = "android"))]
fn session_state_plan_for(
    prompt: &str,
    session_id: Option<&str>,
    cache_policy: Option<&str>,
    model_path: Option<&str>,
    n_ctx: u32,
    n_batch: u32,
    n_gpu_layers: u32,
    llama_split_mode: &LlamaSplitModeArg,
    llama_main_gpu: i32,
    llama_devices: Option<&str>,
) -> Option<SessionStateCachePlan> {
    if !session_state_cache_enabled() {
        return None;
    }

    let session_id = session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let policy = SessionStateCachePolicy::parse(cache_policy);
    if matches!(
        policy,
        SessionStateCachePolicy::Bypass | SessionStateCachePolicy::Unknown
    ) {
        let session_hash = sha256_str_hex(session_id);
        return Some(SessionStateCachePlan {
            policy,
            session_hash_short: session_hash[..12].to_string(),
            session_dir: session_state_cache_root().join("disabled"),
            state_path: session_state_cache_root().join("disabled").join("noop"),
            model_key_hash: String::new(),
            prompt_hash: String::new(),
        });
    }

    let session_hash = sha256_str_hex(session_id);
    let session_hash_short = session_hash[..12].to_string();
    let model_key_hash = model_cache_key(
        model_path,
        n_ctx,
        n_batch,
        n_gpu_layers,
        llama_split_mode,
        llama_main_gpu,
        llama_devices,
    );
    let prompt_hash = sha256_hex(&["prompt-cache-v1", prompt]);
    let session_dir = session_state_cache_root().join(&session_hash);
    let state_path = session_dir.join(format!("{}-{}.state", model_key_hash, prompt_hash));

    Some(SessionStateCachePlan {
        policy,
        session_hash_short,
        session_dir,
        state_path,
        model_key_hash,
        prompt_hash,
    })
}

#[cfg(not(target_os = "android"))]
fn clear_session_state_dir(session_dir: &Path) -> Result<bool> {
    if !session_dir.exists() {
        return Ok(false);
    }

    std_fs::remove_dir_all(session_dir)
        .map(|_| true)
        .map_err(|e| {
            anyhow!(
                "Failed to remove session state dir {}: {}",
                session_dir.display(),
                e
            )
        })
}

#[cfg(not(target_os = "android"))]
fn session_state_meta_path(state_path: &Path) -> PathBuf {
    let meta_name = state_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.meta"))
        .unwrap_or_else(|| "state.meta".to_string());
    state_path.with_file_name(meta_name)
}

#[cfg(not(target_os = "android"))]
fn file_sha256_hex(path: &Path) -> Result<String> {
    let mut file = std_fs::File::open(path)
        .map_err(|e| anyhow!("Failed to open checkpoint {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|e| anyhow!("Failed to read checkpoint {}: {}", path.display(), e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(not(target_os = "android"))]
fn write_session_state_meta(plan: &SessionStateCachePlan) -> Result<()> {
    let meta = SessionStateCheckpointMeta {
        version: SESSION_STATE_META_VERSION,
        model_key_hash: plan.model_key_hash.clone(),
        prompt_hash: plan.prompt_hash.clone(),
        state_sha256: file_sha256_hex(&plan.state_path)?,
    };
    let meta_path = session_state_meta_path(&plan.state_path);
    let encoded = serde_json::to_vec(&meta)?;
    std_fs::write(&meta_path, encoded).map_err(|e| {
        anyhow!(
            "Failed to write checkpoint meta {}: {}",
            meta_path.display(),
            e
        )
    })
}

#[cfg(not(target_os = "android"))]
fn validate_session_state_meta(plan: &SessionStateCachePlan) -> Result<bool> {
    let meta_path = session_state_meta_path(&plan.state_path);
    let encoded = match std_fs::read(&meta_path) {
        Ok(encoded) => encoded,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(anyhow!(
                "Failed to read checkpoint meta {}: {}",
                meta_path.display(),
                e
            ))
        }
    };
    let meta: SessionStateCheckpointMeta = serde_json::from_slice(&encoded)?;
    if meta.version != SESSION_STATE_META_VERSION
        || meta.model_key_hash != plan.model_key_hash
        || meta.prompt_hash != plan.prompt_hash
    {
        return Ok(false);
    }

    Ok(meta.state_sha256 == file_sha256_hex(&plan.state_path)?)
}

#[cfg(not(target_os = "android"))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct StateCheckpointFile {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

#[cfg(not(target_os = "android"))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct StateCheckpointQuotaReport {
    bytes_current: u64,
    evicted_files: u64,
}

#[cfg(not(target_os = "android"))]
fn collect_state_checkpoint_files(root: &Path) -> Result<Vec<StateCheckpointFile>> {
    fn visit(dir: &Path, files: &mut Vec<StateCheckpointFile>) -> Result<()> {
        let entries = match std_fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(anyhow!(
                    "Failed to read session state dir {}: {}",
                    dir.display(),
                    e
                ))
            }
        };

        for entry in entries {
            let entry = entry.map_err(|e| {
                anyhow!(
                    "Failed to read session state entry in {}: {}",
                    dir.display(),
                    e
                )
            })?;
            let path = entry.path();
            let metadata = entry.metadata().map_err(|e| {
                anyhow!(
                    "Failed to stat session state entry {}: {}",
                    path.display(),
                    e
                )
            })?;
            if metadata.is_dir() {
                visit(&path, files)?;
                continue;
            }
            if metadata.is_file()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| value == "state")
                    .unwrap_or(false)
            {
                files.push(StateCheckpointFile {
                    path,
                    bytes: metadata.len(),
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                });
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, &mut files)?;
    Ok(files)
}

#[cfg(not(target_os = "android"))]
fn enforce_session_state_quota(root: &Path) -> Result<StateCheckpointQuotaReport> {
    let max_bytes = session_state_cache_max_bytes();
    set_worker_state_checkpoint_max_bytes(max_bytes);

    let mut files = collect_state_checkpoint_files(root)?;
    let mut bytes_current = files.iter().map(|file| file.bytes).sum::<u64>();
    let mut evicted_files = 0u64;

    if max_bytes > 0 && bytes_current > max_bytes {
        files.sort_by_key(|file| file.modified);
        for file in files {
            if bytes_current <= max_bytes {
                break;
            }
            match std_fs::remove_file(&file.path) {
                Ok(()) => {
                    let meta_path = session_state_meta_path(&file.path);
                    if let Err(e) = std_fs::remove_file(&meta_path) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            record_worker_state_checkpoint_error();
                            warn!(
                                "Failed to remove worker session state checkpoint metadata during quota enforcement: {}",
                                e
                            );
                        }
                    }
                    bytes_current = bytes_current.saturating_sub(file.bytes);
                    evicted_files = evicted_files.saturating_add(1);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    bytes_current = bytes_current.saturating_sub(file.bytes);
                }
                Err(e) => {
                    record_worker_state_checkpoint_error();
                    warn!(
                        "Failed to remove worker session state checkpoint during quota enforcement: {}",
                        e
                    );
                }
            }
        }
    }

    if evicted_files > 0 {
        record_worker_state_checkpoint_quota_eviction(evicted_files);
    }
    set_worker_state_checkpoint_bytes_current(bytes_current);

    Ok(StateCheckpointQuotaReport {
        bytes_current,
        evicted_files,
    })
}

#[cfg(not(target_os = "android"))]
fn forward_blocking_stream_error(tx: &mpsc::Sender<Result<String>>, error: anyhow::Error) -> bool {
    warn!("Worker session state streaming inference failed: {}", error);
    tx.blocking_send(Err(error)).is_ok()
}

#[allow(dead_code)] // LLM engine implementation for llama.cpp (embedded mode)
#[derive(Clone)] // Enable cloning for shared instance usage
pub struct LlamaEngine {
    pub models: Arc<RwLock<Vec<super::ModelInfo>>>,
    pub models_name: Vec<String>,
    pub model_path: Option<String>,
    pub llama_mmproj_path: Option<String>,
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_gpu_layers: u32,
    pub llama_split_mode: LlamaSplitModeArg,
    pub llama_main_gpu: i32,
    pub llama_devices: Option<String>,
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
    #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
    pub cached_mtmd: Option<Arc<Mutex<MtmdContext>>>,
    #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
    pub cached_mmproj_path: Option<String>,
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
    /// Hint for max tokens to spend on thinking content (Anthropic extended thinking).
    /// The model uses this as guidance; actual thinking token count depends on model output.
    #[allow(dead_code)]
    pub thinking_budget_tokens: Option<usize>,
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
            thinking_budget_tokens: None,
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
        Ok(format!(
            "llama-cpp-2 response for: {} ({} tokens)",
            &prompt[..prompt.len().min(30)],
            max_tokens
        ))
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
            let model_path = self
                .model_path
                .as_ref()
                .ok_or_else(|| anyhow!("Model path not set"))?
                .clone();

            let resolved_model_path = self.validate_model_path(&model_path)?;
            let resolved_model_path_str = resolved_model_path.to_string_lossy().to_string();
            let resolved_mmproj_path = match self.llama_mmproj_path.as_deref() {
                Some(path) => Some(self.validate_model_path(path)?),
                None => None,
            };
            let resolved_mmproj_path_str = resolved_mmproj_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string());

            #[cfg(not(feature = "multimodal"))]
            if resolved_mmproj_path_str.is_some() {
                return Err(anyhow!(
                    "LLAMA multimodal projector was configured, but gpuf-c was built without the 'multimodal' feature"
                ));
            }

            // Check if model is already cached AND matches current path
            if let Some(ref cached_path) = self.cached_model_path {
                let mmproj_matches = {
                    #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
                    {
                        self.cached_mmproj_path == resolved_mmproj_path_str
                    }
                    #[cfg(not(all(not(target_os = "android"), feature = "multimodal")))]
                    {
                        true
                    }
                };

                if cached_path == &resolved_model_path_str
                    && self.cached_model.is_some()
                    && mmproj_matches
                {
                    info!(
                        "Model already loaded and cached: {}",
                        resolved_model_path_str
                    );
                    return Ok(());
                } else if cached_path != &resolved_model_path_str {
                    // Model path changed, clear old cache
                    warn!(
                        "Model path changed from {} to {}, clearing cache",
                        cached_path, resolved_model_path_str
                    );
                    self.clear_cache();
                }
            }
            let n_gpu_layers = self.n_gpu_layers;
            let llama_split_mode = self.llama_split_mode.clone();
            let llama_main_gpu = self.llama_main_gpu;
            let llama_devices = self.llama_devices.clone();
            let model_path_for_closure = resolved_model_path_str.clone();
            let model_path_for_cache = model_path_for_closure.clone();
            let mmproj_path_for_closure = resolved_mmproj_path_str.clone();
            let mmproj_path_for_cache = resolved_mmproj_path_str.clone();

            info!(
                "Loading and caching llama-cpp-2 model: {}",
                model_path_for_closure
            );

            // Run model loading in blocking thread
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            let (backend, model, mtmd_context) = tokio::task::spawn_blocking(move || {
                // Use global backend singleton - initialize only once
                let backend = LLAMA_BACKEND
                    .get_or_init(|| {
                        info!("Initializing Llama backend (first time only)");
                        match LlamaBackend::init() {
                            Ok(b) => Arc::new(b),
                            Err(e) => {
                                warn!("Failed to initialize Llama backend: {:?}", e);
                                panic!("Cannot initialize Llama backend: {:?}", e);
                            }
                        }
                    })
                    .clone();

                use crate::util::nvswitch_check;
                use llama_cpp_2::model::params::LlamaSplitMode as LlamaCppSplitMode;

                // Check NVSwitch availability before CUDA init (HGX/A100/A800)
                if !nvswitch_check::check_hgx_nvswitch_available() {
                    return Err(anyhow!(
                        "NVSwitch/HGX not ready - CUDA will fail with error 802. \
                         Run: sudo systemctl start nvidia-fabricmanager"
                    ));
                }

                let split_mode = match llama_split_mode {
                    LlamaSplitModeArg::None => LlamaCppSplitMode::None,
                    LlamaSplitModeArg::Layer => LlamaCppSplitMode::Layer,
                    LlamaSplitModeArg::Row => LlamaCppSplitMode::Row,
                };

                // Build base model params step by step
                let mut model_params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
                model_params = model_params.with_split_mode(split_mode);
                model_params = model_params.with_main_gpu(llama_main_gpu);

                // Apply devices if provided (rebuild on success to avoid move issues)
                if let Some(ref devs) = llama_devices {
                    let devs = devs.trim();
                    if !devs.is_empty() {
                        if let Ok(devices) = devs
                            .split(',')
                            .map(|s| s.trim().parse::<usize>())
                            .collect::<Result<Vec<_>, _>>()
                        {
                            // Rebuild params with devices to avoid with_devices move issue
                            let new_params = LlamaModelParams::default()
                                .with_n_gpu_layers(n_gpu_layers)
                                .with_split_mode(split_mode)
                                .with_main_gpu(llama_main_gpu)
                                .with_devices(&devices);
                            if let Ok(p) = new_params {
                                model_params = p;
                                info!("Applied llama_devices: {:?}", devices);
                            } else {
                                warn!("Failed to apply llama_devices: {:?}", devices);
                            }
                        } else {
                            warn!("Failed to parse llama_devices: '{}'", devs);
                        }
                    }
                }

                let model =
                    LlamaModel::load_from_file(&*backend, &model_path_for_closure, &model_params)
                        .map_err(|e| anyhow!("Failed to load model: {:?}", e))?;

                let mtmd_context = match mmproj_path_for_closure {
                    Some(mmproj_path) => {
                        let mut mtmd_params = MtmdContextParams::default();
                        mtmd_params.use_gpu = true;
                        mtmd_params.print_timings = false;
                        let context =
                            MtmdContext::init_from_file(&mmproj_path, &model, &mtmd_params)
                                .map_err(|e| {
                                    anyhow!(
                                        "Failed to load multimodal projector {}: {:?}",
                                        mmproj_path,
                                        e
                                    )
                                })?;
                        if !context.support_vision() {
                            warn!("Multimodal projector loaded but vision support is not reported");
                        }
                        Some(context)
                    }
                    None => None,
                };

                Ok::<(Arc<LlamaBackend>, LlamaModel, Option<MtmdContext>), anyhow::Error>((
                    backend,
                    model,
                    mtmd_context,
                ))
            })
            .await??;

            #[cfg(not(feature = "multimodal"))]
            let (backend, model) = tokio::task::spawn_blocking(move || {
                // Use global backend singleton - initialize only once
                let backend = LLAMA_BACKEND
                    .get_or_init(|| {
                        info!("Initializing Llama backend (first time only)");
                        match LlamaBackend::init() {
                            Ok(b) => Arc::new(b),
                            Err(e) => {
                                warn!("Failed to initialize Llama backend: {:?}", e);
                                panic!("Cannot initialize Llama backend: {:?}", e);
                            }
                        }
                    })
                    .clone();

                use crate::util::nvswitch_check;
                use llama_cpp_2::model::params::LlamaSplitMode as LlamaCppSplitMode;

                if !nvswitch_check::check_hgx_nvswitch_available() {
                    return Err(anyhow!(
                        "NVSwitch/HGX not ready - CUDA will fail with error 802. \
                         Run: sudo systemctl start nvidia-fabricmanager"
                    ));
                }

                let split_mode = match llama_split_mode {
                    LlamaSplitModeArg::None => LlamaCppSplitMode::None,
                    LlamaSplitModeArg::Layer => LlamaCppSplitMode::Layer,
                    LlamaSplitModeArg::Row => LlamaCppSplitMode::Row,
                };

                let mut model_params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
                model_params = model_params.with_split_mode(split_mode);
                model_params = model_params.with_main_gpu(llama_main_gpu);

                if let Some(ref devs) = llama_devices {
                    let devs = devs.trim();
                    if !devs.is_empty() {
                        if let Ok(devices) = devs
                            .split(',')
                            .map(|s| s.trim().parse::<usize>())
                            .collect::<Result<Vec<_>, _>>()
                        {
                            let new_params = LlamaModelParams::default()
                                .with_n_gpu_layers(n_gpu_layers)
                                .with_split_mode(split_mode)
                                .with_main_gpu(llama_main_gpu)
                                .with_devices(&devices);
                            if let Ok(p) = new_params {
                                model_params = p;
                                info!("Applied llama_devices: {:?}", devices);
                            } else {
                                warn!("Failed to apply llama_devices: {:?}", devices);
                            }
                        } else {
                            warn!("Failed to parse llama_devices: '{}'", devs);
                        }
                    }
                }

                let model =
                    LlamaModel::load_from_file(&*backend, &model_path_for_closure, &model_params)
                        .map_err(|e| anyhow!("Failed to load model: {:?}", e))?;

                Ok::<(Arc<LlamaBackend>, LlamaModel), anyhow::Error>((backend, model))
            })
            .await??;

            // Cache the components and store the model path
            self.cached_backend = Some(backend);
            self.cached_model = Some(Arc::new(Mutex::new(model)));
            self.cached_model_path = Some(model_path_for_cache.clone());
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            {
                self.cached_mtmd = mtmd_context.map(|ctx| Arc::new(Mutex::new(ctx)));
                self.cached_mmproj_path = mmproj_path_for_cache;
            }
            self.is_initialized = true;

            info!(
                "Model successfully loaded and cached: {}",
                model_path_for_cache
            );
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
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            {
                self.cached_mtmd = None;
                self.cached_mmproj_path = None;
            }
            self.is_initialized = false;
            info!("Model cache cleared");
        }
    }

    /// Generate text using cached model (inference only)
    /// Returns (generated_text, prompt_tokens, completion_tokens)
    pub async fn generate_with_cached_model(
        &self,
        prompt: &str,
        max_tokens: usize,
    ) -> Result<(String, usize, usize)> {
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
        let _ = sampling;

        #[cfg(target_os = "android")]
        {
            // Android: Simulated response
            warn!("Android SDK: Using simulated response");
            let text = format!(
                "Android SDK response for: {} (simulated, {} tokens)",
                &prompt[..prompt.len().min(30)],
                max_tokens
            );
            Ok((text, 10, 20)) // Simulated token counts
        }

        #[cfg(not(target_os = "android"))]
        {
            // Client: Real inference using cached model
            info!("Client: Executing inference with cached model");

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
            let n_batch = self.n_batch;
            let sampling = sampling.clone();

            // Run inference in blocking thread
            tokio::task::spawn_blocking(move || {
                use llama_cpp_2::llama_batch::LlamaBatch;
                use llama_cpp_2::model::AddBos;
                use llama_cpp_2::sampling::LlamaSampler;

                let context_params = LlamaContextParams::default()
                    .with_n_ctx(NonZeroU32::new(n_ctx))
                    .with_n_batch(n_batch);

                // Lock model and create context with proper lifetime
                let model_guard = model
                    .lock()
                    .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;

                let mut context = model_guard
                    .new_context(&*backend, context_params)
                    .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;

                // Tokenize the prompt
                let tokens = model_guard
                    .str_to_token(&prompt, AddBos::Always)
                    .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;

                // Create batch and add tokens
                let mut batch = LlamaBatch::new(tokens.len(), 1);
                for (i, token) in tokens.iter().enumerate() {
                    let is_last = i == tokens.len() - 1;
                    batch
                        .add(*token, i as i32, &[0], is_last)
                        .map_err(|e| anyhow!("Failed to add token to batch: {:?}", e))?;
                }

                // Decode tokens (process prompt)
                context
                    .decode(&mut batch)
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
                    let mut token_decoder = encoding_rs::UTF_8.new_decoder();
                    let piece = model_guard
                        .token_to_piece(new_token, &mut token_decoder, true, None)
                        .ok();

                    debug!("Token {}: id={}, text={:?}", i, new_token, piece);

                    // Check for EOS token
                    if new_token == model_guard.token_eos() {
                        break;
                    }
                    // Convert token to string and append
                    if let Some(piece) = piece {
                        // Check for stop sequences (ChatML, Llama3, etc.)
                        if piece.contains("<|im_end|>")
                            || piece.contains("<|eot_id|>")
                            || piece.contains("<|end_of_text|>")
                            || piece.contains("</s>")
                        {
                            break;
                        }
                        output_text.push_str(&piece);
                    }

                    output_tokens.push(new_token);

                    // Prepare next batch with single token at correct position
                    let mut next_batch = LlamaBatch::new(1, 1);
                    next_batch
                        .add(new_token, n_cur as i32, &[0], true)
                        .map_err(|e| anyhow!("Failed to add token: {:?}", e))?;

                    // Decode next token
                    context
                        .decode(&mut next_batch)
                        .map_err(|e| anyhow!("Failed to decode token: {:?}", e))?;

                    // Increment position for next token
                    n_cur += 1;
                }

                // Return text with token counts
                let prompt_token_count = tokens.len();
                let completion_token_count = output_tokens.len();
                Ok((output_text, prompt_token_count, completion_token_count))
            })
            .await?
        }
    }

    pub async fn render_chat_prompt(
        &self,
        messages: &[(String, String)],
        add_generation_prompt: bool,
    ) -> Result<String> {
        #[cfg(target_os = "android")]
        {
            let _ = add_generation_prompt;
            let mut prompt = String::new();
            for (role, content) in messages {
                prompt.push_str(role);
                prompt.push_str(": ");
                prompt.push_str(content);
                prompt.push('\n');
            }
            return Ok(prompt);
        }

        #[cfg(not(target_os = "android"))]
        {
            let model = self
                .cached_model
                .as_ref()
                .ok_or_else(|| anyhow!("Model not loaded - call load_model() first"))?
                .clone();
            let messages = messages.to_vec();

            tokio::task::spawn_blocking(move || {
                use llama_cpp_2::model::LlamaChatMessage;

                let model_guard = model
                    .lock()
                    .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;

                let tmpl = model_guard
                    .chat_template(None)
                    .map_err(|e| anyhow!("Failed to get chat template: {:?}", e))?;

                let mut chat = Vec::with_capacity(messages.len());
                for (role, content) in messages {
                    chat.push(
                        LlamaChatMessage::new(role, content)
                            .map_err(|e| anyhow!("Failed to build chat message: {:?}", e))?,
                    );
                }

                model_guard
                    .apply_chat_template(&tmpl, &chat, add_generation_prompt)
                    .map_err(|e| anyhow!("Failed to apply chat template: {:?}", e))
            })
            .await?
        }
    }

    #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
    pub async fn generate_multimodal_with_cached_model_sampling(
        &self,
        prompt: &str,
        images: Vec<Vec<u8>>,
        max_tokens: usize,
        sampling: &SamplingParams,
    ) -> Result<(String, usize, usize)> {
        if !self.is_initialized {
            return Err(anyhow!("Engine not initialized - call load_model() first"));
        }
        if images.is_empty() {
            return Err(anyhow!("multimodal generation requires at least one image"));
        }

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
        let mtmd = self
            .cached_mtmd
            .as_ref()
            .ok_or_else(|| {
                anyhow!("Multimodal projector not loaded; start gpuf-c with --llama-mmproj-path")
            })?
            .clone();

        let mut prompt = prompt.to_string();
        let marker = mtmd_default_marker();
        let marker_count = prompt.matches(marker).count();
        if marker_count == 0 {
            let prefix = marker.repeat(images.len());
            prompt = format!("{prefix}{prompt}");
        } else if marker_count != images.len() {
            return Err(anyhow!(
                "multimodal prompt contains {} media markers but {} images were provided",
                marker_count,
                images.len()
            ));
        }

        let n_ctx = self.n_ctx;
        let n_batch = self.n_batch;
        let sampling = sampling.clone();

        tokio::task::spawn_blocking(move || {
            use llama_cpp_2::llama_batch::LlamaBatch;
            use llama_cpp_2::sampling::LlamaSampler;

            let context_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(n_ctx))
                .with_n_batch(n_batch);

            let model_guard = model
                .lock()
                .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;
            let mtmd_guard = mtmd
                .lock()
                .map_err(|e| anyhow!("Failed to lock multimodal context: {:?}", e))?;

            let mut context = model_guard
                .new_context(&*backend, context_params)
                .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;

            let bitmaps = images
                .iter()
                .map(|image| {
                    MtmdBitmap::from_buffer(&mtmd_guard, image)
                        .map_err(|e| anyhow!("Failed to decode image for mtmd: {:?}", e))
                })
                .collect::<Result<Vec<_>>>()?;
            let bitmap_refs = bitmaps.iter().collect::<Vec<_>>();

            let input = MtmdInputText {
                text: prompt,
                add_special: true,
                parse_special: true,
            };
            let chunks = mtmd_guard
                .tokenize(input, bitmap_refs.as_slice())
                .map_err(|e| anyhow!("Failed to tokenize multimodal prompt: {:?}", e))?;
            let prompt_token_count = chunks.total_tokens();
            let n_past = chunks
                .eval_chunks(&mtmd_guard, &context, 0, 0, n_batch as i32, true)
                .map_err(|e| anyhow!("Failed to eval multimodal chunks: {:?}", e))?;

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
            let mut output_tokens = Vec::new();
            let mut output_text = String::new();
            let mut n_cur = n_past;

            for i in 0..max_tokens {
                let new_token = sampler.sample(&context, -1);
                sampler.accept(new_token);

                if new_token == model_guard.token_eos() {
                    break;
                }

                let mut token_decoder = encoding_rs::UTF_8.new_decoder();
                if let Ok(piece) =
                    model_guard.token_to_piece(new_token, &mut token_decoder, true, None)
                {
                    debug!("Multimodal token {}: id={}, text={:?}", i, new_token, piece);
                    if piece.contains("<|im_end|>")
                        || piece.contains("<|eot_id|>")
                        || piece.contains("<|end_of_text|>")
                        || piece.contains("</s>")
                    {
                        break;
                    }
                    output_text.push_str(&piece);
                }

                output_tokens.push(new_token);

                let mut next_batch = LlamaBatch::new(1, 1);
                next_batch
                    .add(new_token, n_cur, &[0], true)
                    .map_err(|e| anyhow!("Failed to add multimodal token: {:?}", e))?;
                context
                    .decode(&mut next_batch)
                    .map_err(|e| anyhow!("Failed to decode multimodal token: {:?}", e))?;
                n_cur += 1;
            }

            Ok((output_text, prompt_token_count, output_tokens.len()))
        })
        .await?
    }

    #[cfg(any(target_os = "android", not(feature = "multimodal")))]
    pub async fn generate_multimodal_with_cached_model_sampling(
        &self,
        _prompt: &str,
        _images: Vec<Vec<u8>>,
        _max_tokens: usize,
        _sampling: &SamplingParams,
    ) -> Result<(String, usize, usize)> {
        Err(anyhow!(
            "Multimodal generation is not available in this gpuf-c build"
        ))
    }

    pub async fn generate_embeddings(
        &self,
        inputs: Vec<String>,
        normalize: bool,
    ) -> Result<(Vec<Vec<f32>>, u32)> {
        if !self.is_initialized {
            return Err(anyhow!("Engine not initialized - call load_model() first"));
        }

        #[cfg(target_os = "android")]
        {
            let _ = (inputs, normalize);
            Err(anyhow!(
                "Embedding inference is not supported on Android yet"
            ))
        }

        #[cfg(not(target_os = "android"))]
        {
            if inputs.is_empty() {
                return Err(anyhow!("embedding input must not be empty"));
            }

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

            let n_ctx = self.n_ctx;
            let n_batch = self.n_batch;

            let (embeddings, prompt_tokens) =
                tokio::task::spawn_blocking(move || -> Result<(Vec<Vec<f32>>, usize)> {
                    use llama_cpp_2::llama_batch::LlamaBatch;
                    use llama_cpp_2::model::AddBos;

                    let model_guard = model
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;
                    let mut output = Vec::with_capacity(inputs.len());
                    let mut total_tokens = 0usize;

                    for input in inputs {
                        let tokens = model_guard
                            .str_to_token(&input, AddBos::Always)
                            .map_err(|e| anyhow!("Failed to tokenize embedding input: {:?}", e))?;
                        if tokens.is_empty() {
                            return Err(anyhow!("embedding input produced no tokens"));
                        }
                        if tokens.len() > n_ctx as usize {
                            return Err(anyhow!(
                                "embedding input has {} tokens, exceeding n_ctx {}",
                                tokens.len(),
                                n_ctx
                            ));
                        }

                        let embedding_batch = n_batch.max(tokens.len() as u32);
                        let context_params = || {
                            LlamaContextParams::default()
                                .with_n_ctx(NonZeroU32::new(n_ctx))
                                .with_n_batch(embedding_batch)
                                .with_n_ubatch(embedding_batch)
                                .with_embeddings(true)
                                .with_pooling_type(LlamaPoolingType::Mean)
                                .with_attention_type(LlamaAttentionType::NonCausal)
                        };

                        let mut context = model_guard
                            .new_context(&*backend, context_params())
                            .map_err(|e| anyhow!("Failed to create embedding context: {:?}", e))?;

                        let mut batch = LlamaBatch::new(tokens.len(), 1);
                        for (pos, token) in tokens.iter().enumerate() {
                            batch
                                .add(*token, pos as i32, &[0], false)
                                .map_err(|e| anyhow!("Failed to add embedding token: {:?}", e))?;
                        }

                        if let Err(encode_err) = context.encode(&mut batch) {
                            let mut decode_context = model_guard
                                .new_context(&*backend, context_params())
                                .map_err(|e| {
                                    anyhow!("Failed to create fallback embedding context: {:?}", e)
                                })?;
                            let mut decode_batch = LlamaBatch::new(tokens.len(), 1);
                            for (pos, token) in tokens.iter().enumerate() {
                                decode_batch
                                    .add(*token, pos as i32, &[0], false)
                                    .map_err(|e| {
                                        anyhow!("Failed to add fallback embedding token: {:?}", e)
                                    })?;
                            }
                            decode_context
                                .decode(&mut decode_batch)
                                .map_err(|decode_err| {
                                    anyhow!(
                                        "Failed to run embedding batch: encode={:?}, decode={:?}",
                                        encode_err,
                                        decode_err
                                    )
                                })?;
                            context = decode_context;
                        }

                        let mut embedding = context
                            .embeddings_seq_ith(0)
                            .map_err(|e| anyhow!("Failed to read embedding: {:?}", e))?
                            .to_vec();

                        if normalize {
                            let norm = embedding
                                .iter()
                                .map(|value| value * value)
                                .sum::<f32>()
                                .sqrt();
                            if norm > 0.0 {
                                for value in &mut embedding {
                                    *value /= norm;
                                }
                            }
                        }

                        total_tokens = total_tokens.saturating_add(tokens.len());
                        output.push(embedding);
                    }

                    Ok((output, total_tokens))
                })
                .await??;

            Ok((embeddings, u32::try_from(prompt_tokens).unwrap_or(u32::MAX)))
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
            let n_batch = self.n_batch;
            let sampling = sampling.clone();

            let (tx, rx) = mpsc::channel::<Result<String>>(64);
            info!(
                prompt_bytes = prompt.len(),
                max_tokens, "Starting llama cached-model stream"
            );

            tokio::task::spawn_blocking(move || {
                let result = (|| {
                    use llama_cpp_2::llama_batch::LlamaBatch;
                    use llama_cpp_2::model::AddBos;
                    use llama_cpp_2::sampling::LlamaSampler;

                    let context_params = LlamaContextParams::default()
                        .with_n_ctx(NonZeroU32::new(n_ctx))
                        .with_n_batch(n_batch);

                    let model_guard = model
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;
                    let mut context = model_guard
                        .new_context(&*backend, context_params)
                        .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;

                    let tokens = model_guard
                        .str_to_token(&prompt, AddBos::Always)
                        .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;
                    info!(
                        prompt_tokens = tokens.len(),
                        "Llama prompt tokenized for cached-model stream"
                    );

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
                    info!(max_tokens, "Llama generation loop starting");

                    let mut emitted_tokens: usize = 0;
                    let mut n_cur = tokens.len();
                    for _i in 0..max_tokens {
                        let new_token = sampler.sample(&context, -1);
                        sampler.accept(new_token);

                        if new_token == model_guard.token_eos() {
                            break;
                        }
                        let mut token_decoder = encoding_rs::UTF_8.new_decoder();
                        if let Ok(piece) =
                            model_guard.token_to_piece(new_token, &mut token_decoder, true, None)
                        {
                            if piece.contains("<|im_end|>")
                                || piece.contains("<|eot_id|>")
                                || piece.contains("<|end_of_text|>")
                                || piece.contains("</s>")
                            {
                                break;
                            }

                            if tx.blocking_send(Ok(piece)).is_err() {
                                warn!(
                                    emitted_tokens,
                                    "Llama stream receiver dropped while sending token"
                                );
                                break;
                            }
                            emitted_tokens += 1;
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

                    info!(emitted_tokens, "Llama generation loop finished");
                    Ok::<(), anyhow::Error>(())
                })();
                if let Err(error) = result {
                    forward_blocking_stream_error(&tx, error);
                }
            });

            Ok(ReceiverStream::new(rx))
        }
    }

    pub async fn stream_with_session_state_sampling(
        &self,
        prompt: &str,
        max_tokens: usize,
        sampling: &SamplingParams,
        session_id: Option<&str>,
        cache_policy: Option<&str>,
    ) -> Result<impl Stream<Item = Result<String>> + Send + 'static> {
        if !self.is_initialized {
            return Err(anyhow!("Engine not initialized - call load_model() first"));
        }

        #[cfg(target_os = "android")]
        {
            let _ = (session_id, cache_policy);
            return self
                .stream_with_cached_model_sampling(prompt, max_tokens, sampling)
                .await;
        }

        #[cfg(not(target_os = "android"))]
        {
            let cache_plan = session_state_plan_for(
                prompt,
                session_id,
                cache_policy,
                self.cached_model_path
                    .as_deref()
                    .or(self.model_path.as_deref()),
                self.n_ctx,
                self.n_batch,
                self.n_gpu_layers,
                &self.llama_split_mode,
                self.llama_main_gpu,
                self.llama_devices.as_deref(),
            );

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
            let n_batch = self.n_batch;
            let sampling = sampling.clone();

            let (tx, rx) = mpsc::channel::<Result<String>>(64);
            info!(
                session_id = session_id.unwrap_or("none"),
                cache_policy = cache_policy.unwrap_or("none"),
                prompt_bytes = prompt.len(),
                max_tokens,
                "Starting llama session-state stream"
            );

            tokio::task::spawn_blocking(move || {
                let result = (|| {
                    use llama_cpp_2::llama_batch::LlamaBatch;
                    use llama_cpp_2::model::AddBos;
                    use llama_cpp_2::sampling::LlamaSampler;

                    if let Some(cache_plan) = cache_plan.as_ref() {
                        if matches!(
                            cache_plan.policy,
                            SessionStateCachePolicy::Bypass | SessionStateCachePolicy::Unknown
                        ) {
                            debug!(
                                session = %cache_plan.session_hash_short,
                                policy = cache_plan.policy.as_str(),
                                "Session state cache skipped by policy"
                            );
                        }

                        if matches!(cache_plan.policy, SessionStateCachePolicy::Reset) {
                            match clear_session_state_dir(&cache_plan.session_dir) {
                                Ok(true) => {
                                    record_worker_state_checkpoint_reset();
                                    if let Err(e) =
                                        enforce_session_state_quota(&session_state_cache_root())
                                    {
                                        record_worker_state_checkpoint_error();
                                        warn!(
                                            session = %cache_plan.session_hash_short,
                                            "Failed to refresh worker session state quota after reset: {}",
                                            e
                                        );
                                    }
                                    info!(
                                        session = %cache_plan.session_hash_short,
                                        "Cleared worker session state cache"
                                    );
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    record_worker_state_checkpoint_error();
                                    warn!(
                                        session = %cache_plan.session_hash_short,
                                        "Failed to clear worker session state cache: {}",
                                        e
                                    );
                                }
                            }
                        }
                    }

                    let context_params = LlamaContextParams::default()
                        .with_n_ctx(NonZeroU32::new(n_ctx))
                        .with_n_batch(n_batch);

                    let model_guard = model
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;
                    let mut context = model_guard
                        .new_context(&*backend, context_params)
                        .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;

                    let tokens = model_guard
                        .str_to_token(&prompt, AddBos::Always)
                        .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;
                    info!(
                        prompt_tokens = tokens.len(),
                        "Llama prompt tokenized for session-state stream"
                    );

                    let mut restored_prefix_len = 0usize;
                    if let Some(cache_plan) = cache_plan.as_ref() {
                        if cache_plan.policy.should_load()
                            && tokens.len() > 1
                            && cache_plan.state_path.exists()
                        {
                            let meta_ok = match validate_session_state_meta(cache_plan) {
                                Ok(valid) => valid,
                                Err(e) => {
                                    record_worker_state_checkpoint_error();
                                    warn!(
                                        session = %cache_plan.session_hash_short,
                                        "Failed to validate worker session state checkpoint metadata: {}",
                                        e
                                    );
                                    false
                                }
                            };
                            if meta_ok {
                                let max_state_tokens = (n_ctx as usize).max(tokens.len());
                                match context
                                    .state_load_file(&cache_plan.state_path, max_state_tokens)
                                {
                                    Ok(cached_tokens)
                                        if cached_tokens.len() + 1 == tokens.len()
                                            && cached_tokens.as_slice()
                                                == &tokens[..cached_tokens.len()] =>
                                    {
                                        restored_prefix_len = cached_tokens.len();
                                        record_worker_state_checkpoint_hit();
                                        debug!(
                                            session = %cache_plan.session_hash_short,
                                            model_key = %cache_plan.model_key_hash[..12],
                                            prompt = %cache_plan.prompt_hash[..12],
                                            restored_tokens = restored_prefix_len,
                                            "Restored worker session state checkpoint"
                                        );
                                    }
                                    Ok(cached_tokens) => {
                                        record_worker_state_checkpoint_miss();
                                        debug!(
                                            session = %cache_plan.session_hash_short,
                                            cached_tokens = cached_tokens.len(),
                                            prompt_tokens = tokens.len(),
                                            "Worker session state checkpoint token prefix mismatch"
                                        );
                                    }
                                    Err(e) => {
                                        record_worker_state_checkpoint_error();
                                        warn!(
                                            session = %cache_plan.session_hash_short,
                                            "Failed to load worker session state checkpoint: {:?}",
                                            e
                                        );
                                    }
                                }
                            } else {
                                record_worker_state_checkpoint_miss();
                                debug!(
                                    session = %cache_plan.session_hash_short,
                                    "Worker session state checkpoint metadata missing or mismatched"
                                );
                            }
                        } else if cache_plan.policy.should_load() {
                            record_worker_state_checkpoint_miss();
                        }
                    }

                    if restored_prefix_len > 0 {
                        let last_token = tokens[restored_prefix_len];
                        let mut batch = LlamaBatch::new(1, 1);
                        batch
                            .add(last_token, restored_prefix_len as i32, &[0], true)
                            .map_err(|e| anyhow!("Failed to add replay token to batch: {:?}", e))?;
                        context.decode(&mut batch).map_err(|e| {
                            anyhow!("Failed to replay session state token: {:?}", e)
                        })?;
                    } else {
                        let mut save_state = false;
                        if let Some(cache_plan) = cache_plan.as_ref() {
                            if cache_plan.policy.should_save() && tokens.len() > 1 {
                                if let Err(e) = std_fs::create_dir_all(&cache_plan.session_dir) {
                                    record_worker_state_checkpoint_error();
                                    warn!(
                                        session = %cache_plan.session_hash_short,
                                        "Failed to create worker session state dir: {}",
                                        e
                                    );
                                } else {
                                    save_state = true;
                                }
                            }
                        }

                        let save_prefix_len = if save_state {
                            tokens.len() - 1
                        } else {
                            tokens.len()
                        };
                        if save_prefix_len > 0 {
                            let mut batch = LlamaBatch::new(save_prefix_len, 1);
                            for (i, token) in tokens[..save_prefix_len].iter().enumerate() {
                                let is_last =
                                    i == save_prefix_len - 1 && save_prefix_len == tokens.len();
                                batch.add(*token, i as i32, &[0], is_last).map_err(|e| {
                                    anyhow!("Failed to add token to batch: {:?}", e)
                                })?;
                            }
                            context
                                .decode(&mut batch)
                                .map_err(|e| anyhow!("Failed to decode batch: {:?}", e))?;
                        }

                        if save_state {
                            let cache_plan = cache_plan
                                .as_ref()
                                .expect("save_state is only true with a cache plan");
                            match context
                                .state_save_file(&cache_plan.state_path, &tokens[..save_prefix_len])
                            {
                                Ok(()) => {
                                    record_worker_state_checkpoint_save();
                                    if let Err(e) = write_session_state_meta(cache_plan) {
                                        record_worker_state_checkpoint_error();
                                        warn!(
                                            session = %cache_plan.session_hash_short,
                                            "Failed to write worker session state checkpoint metadata: {}",
                                            e
                                        );
                                    }
                                    if let Err(e) =
                                        enforce_session_state_quota(&session_state_cache_root())
                                    {
                                        record_worker_state_checkpoint_error();
                                        warn!(
                                            session = %cache_plan.session_hash_short,
                                            "Failed to enforce worker session state quota: {}",
                                            e
                                        );
                                    }
                                    debug!(
                                        session = %cache_plan.session_hash_short,
                                        model_key = %cache_plan.model_key_hash[..12],
                                        prompt = %cache_plan.prompt_hash[..12],
                                        saved_tokens = save_prefix_len,
                                        "Saved worker session state checkpoint"
                                    );
                                }
                                Err(e) => {
                                    record_worker_state_checkpoint_error();
                                    warn!(
                                        session = %cache_plan.session_hash_short,
                                        "Failed to save worker session state checkpoint: {:?}",
                                        e
                                    );
                                }
                            }

                            let last_token = tokens[save_prefix_len];
                            let mut last_batch = LlamaBatch::new(1, 1);
                            last_batch
                                .add(last_token, save_prefix_len as i32, &[0], true)
                                .map_err(|e| {
                                    anyhow!("Failed to add final prompt token: {:?}", e)
                                })?;
                            context.decode(&mut last_batch).map_err(|e| {
                                anyhow!("Failed to decode final prompt token: {:?}", e)
                            })?;
                        }
                    }

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
                    info!(max_tokens, "Llama session-state generation loop starting");

                    let mut emitted_tokens: usize = 0;
                    let mut n_cur = tokens.len();
                    for _i in 0..max_tokens {
                        let new_token = sampler.sample(&context, -1);
                        sampler.accept(new_token);

                        if new_token == model_guard.token_eos() {
                            break;
                        }
                        let mut token_decoder = encoding_rs::UTF_8.new_decoder();
                        if let Ok(piece) =
                            model_guard.token_to_piece(new_token, &mut token_decoder, true, None)
                        {
                            if piece.contains("<|im_end|>")
                                || piece.contains("<|eot_id|>")
                                || piece.contains("<|end_of_text|>")
                                || piece.contains("</s>")
                            {
                                break;
                            }

                            if tx.blocking_send(Ok(piece)).is_err() {
                                warn!(
                                    emitted_tokens,
                                    "Llama session-state stream receiver dropped while sending token"
                                );
                                break;
                            }
                            emitted_tokens += 1;
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

                    info!(
                        emitted_tokens,
                        "Llama session-state generation loop finished"
                    );
                    Ok::<(), anyhow::Error>(())
                })();
                if let Err(error) = result {
                    warn!("Llama session-state stream failed: {}", error);
                    forward_blocking_stream_error(&tx, error);
                }
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
            llama_mmproj_path: None,
            n_ctx: 2048,
            n_batch: 4096,
            n_gpu_layers: 99,
            llama_split_mode: LlamaSplitModeArg::Layer,
            llama_main_gpu: 0,
            llama_devices: None,
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
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            cached_mtmd: None,
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            cached_mmproj_path: None,
        }
    }

    pub fn with_runtime_config(
        n_ctx: u32,
        n_batch: u32,
        n_gpu_layers: u32,
        llama_split_mode: LlamaSplitModeArg,
        llama_main_gpu: i32,
        llama_devices: Option<String>,
    ) -> Self {
        let models_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".llama")
            .join("models");

        LlamaEngine {
            models: Arc::new(RwLock::new(Vec::new())),
            models_name: Vec::new(),
            model_path: None,
            llama_mmproj_path: None,
            n_ctx,
            n_batch,
            n_gpu_layers,
            llama_split_mode,
            llama_main_gpu,
            llama_devices,
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
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            cached_mtmd: None,
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            cached_mmproj_path: None,
        }
    }

    pub fn with_config(
        model_path: String,
        llama_mmproj_path: Option<String>,
        n_ctx: u32,
        n_batch: u32,
        n_gpu_layers: u32,
        llama_split_mode: LlamaSplitModeArg,
        llama_main_gpu: i32,
        llama_devices: Option<String>,
    ) -> Self {
        let models_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".llama")
            .join("models");

        LlamaEngine {
            models: Arc::new(RwLock::new(Vec::new())),
            models_name: Vec::new(),
            model_path: Some(model_path.clone()),
            llama_mmproj_path,
            n_ctx,
            n_batch,
            n_gpu_layers,
            llama_split_mode,
            llama_main_gpu,
            llama_devices,
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
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            cached_mtmd: None,
            #[cfg(all(not(target_os = "android"), feature = "multimodal"))]
            cached_mmproj_path: None,
        }
    }

    async fn ensure_initialized(&mut self) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            // On Android, check if SDK has loaded the model
            if self.check_sdk_model_loaded() {
                self.is_initialized = true;
                Ok(())
            } else {
                Err(anyhow!("Android: Model not loaded by SDK yet"))
            }
        }

        #[cfg(not(target_os = "android"))]
        {
            if !self.is_initialized {
                // Use the new separated model loading
                self.initialize_model().await?;
            }
        }
        #[cfg(not(target_os = "android"))]
        Ok(())
    }

    /// Check if SDK has loaded the model (Android only)
    #[cfg(target_os = "android")]
    fn check_sdk_model_loaded(&self) -> bool {
        use crate::GLOBAL_CONTEXT_PTR;
        use crate::GLOBAL_MODEL_PTR;

        let model_ptr = GLOBAL_MODEL_PTR.load(std::sync::atomic::Ordering::SeqCst);
        let context_ptr = GLOBAL_CONTEXT_PTR.load(std::sync::atomic::Ordering::SeqCst);

        !model_ptr.is_null() && !context_ptr.is_null()
    }

    /// Resolve model path with a safe default: relative names must stay in models_dir;
    /// absolute paths are allowed only when they point at an existing model file.
    fn resolve_model_path(&self, path: &str) -> Result<PathBuf> {
        let path_buf = PathBuf::from(path);
        let candidate = if path_buf.is_absolute() {
            path_buf
        } else {
            if path_buf
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(anyhow!("Model path must not contain '..': {}", path));
            }
            self.models_dir.join(path_buf)
        };

        self.validate_model_path_candidate(&candidate, !PathBuf::from(path).is_absolute())
    }

    fn validate_model_path_candidate(
        &self,
        path: &Path,
        must_stay_in_models_dir: bool,
    ) -> Result<PathBuf> {
        if !path.exists() {
            return Err(anyhow!("Model file does not exist: {}", path.display()));
        }
        let canonical = path
            .canonicalize()
            .map_err(|e| anyhow!("Invalid model path '{}': {}", path.display(), e))?;
        if !canonical.is_file() {
            return Err(anyhow!("Model path is not a file: {}", canonical.display()));
        }
        let ext = canonical
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "gguf" | "bin" | "safetensors") {
            return Err(anyhow!(
                "Model path must end with .gguf, .bin, or .safetensors: {}",
                canonical.display()
            ));
        }
        if must_stay_in_models_dir {
            let models_dir = self.models_dir.canonicalize().map_err(|e| {
                anyhow!("Invalid models dir '{}': {}", self.models_dir.display(), e)
            })?;
            if !canonical.starts_with(&models_dir) {
                return Err(anyhow!(
                    "Model path escapes models directory: {}",
                    canonical.display()
                ));
            }
        } else {
            warn!(
                "Using explicit absolute model path: {}",
                canonical.display()
            );
        }
        Ok(canonical)
    }

    fn validate_model_path(&self, path: &str) -> Result<PathBuf> {
        self.resolve_model_path(path)
    }

    async fn generate_response(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        if !self.is_initialized {
            return Err(anyhow!("Llama.cpp engine is not initialized"));
        }

        debug!(
            "Generating response with prompt: {}, max_tokens: {}",
            prompt, max_tokens
        );

        #[cfg(target_os = "android")]
        {
            // Use SDK functions for inference on Android
            if !self.check_sdk_model_loaded() {
                return Err(anyhow!("Android: Model not loaded by SDK"));
            }

            use crate::GLOBAL_CONTEXT_PTR;
            use crate::GLOBAL_MODEL_PTR;
            use std::ffi::CString;
            use std::os::raw::c_char;

            let model_ptr = GLOBAL_MODEL_PTR.load(std::sync::atomic::Ordering::SeqCst);
            let context_ptr = GLOBAL_CONTEXT_PTR.load(std::sync::atomic::Ordering::SeqCst);

            // Convert prompt to C string
            let prompt_cstr =
                CString::new(prompt).map_err(|e| anyhow!("Invalid prompt for C FFI: {}", e))?;

            // Create output buffer (larger buffer for longer responses)
            let mut output = vec![0u8; 8192];

            debug!("Calling SDK inference function");
            let result = crate::gpuf_generate_final_solution_text(
                model_ptr,
                context_ptr,
                prompt_cstr.as_ptr(),
                max_tokens as i32,
                output.as_mut_ptr() as *mut c_char,
                output.len() as i32, // Add missing output_len parameter
            );

            // Check return code (0 = success)
            if result != 0 {
                return Err(anyhow!(
                    "Android: Inference failed with error code: {}",
                    result
                ));
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
                use crate::GLOBAL_CONTEXT_PTR;
                use crate::GLOBAL_MODEL_PTR;

                if self.model_path.is_some() {
                    let model_ptr = GLOBAL_MODEL_PTR.load(std::sync::atomic::Ordering::SeqCst);
                    let context_ptr = GLOBAL_CONTEXT_PTR.load(std::sync::atomic::Ordering::SeqCst);

                    if !model_ptr.is_null() && !context_ptr.is_null() {
                        info!("Android: Model and context already loaded by SDK");
                        self.is_initialized = true;
                    } else {
                        info!(
                            "Android: Model not yet loaded by SDK, waiting for SDK initialization"
                        );
                        // Do not mark as initialized; SDK loading is handled externally.
                    }
                } else {
                    warn!("Android: No model path specified, waiting for SDK to load model");
                }
            }

            #[cfg(not(target_os = "android"))]
            {
                // Non-Android: Normal initialization flow
                if self.model_path.is_none() {
                    warn!("No model path specified, engine will be initialized when model is set");
                    return Ok(());
                }

                let model_path = self
                    .model_path
                    .as_ref()
                    .ok_or_else(|| anyhow!("Model path not set"))?
                    .clone();

                {
                    let mut status = crate::MODEL_STATUS
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock MODEL_STATUS: {:?}", e))?;
                    status.set_loading(&model_path);
                }
                {
                    let mut status = self.loading_status.write().await;
                    *status = "loading".to_string();
                }
                {
                    let mut loading_model = self.current_loading_model.write().await;
                    *loading_model = Some(model_path.clone());
                }

                match self.ensure_initialized().await {
                    Ok(()) => {
                        {
                            let mut status = crate::MODEL_STATUS
                                .lock()
                                .map_err(|e| anyhow!("Failed to lock MODEL_STATUS: {:?}", e))?;
                            status.set_loaded(&model_path);
                        }
                        {
                            let mut status = self.loading_status.write().await;
                            *status = "loaded".to_string();
                        }
                        {
                            let mut loading_model = self.current_loading_model.write().await;
                            *loading_model = None;
                        }
                    }
                    Err(e) => {
                        {
                            let mut status = crate::MODEL_STATUS
                                .lock()
                                .map_err(|e| anyhow!("Failed to lock MODEL_STATUS: {:?}", e))?;
                            status.set_error(&e.to_string());
                        }
                        {
                            let mut status = self.loading_status.write().await;
                            *status = format!("error: {}", e);
                        }
                        {
                            let mut loading_model = self.current_loading_model.write().await;
                            *loading_model = None;
                        }
                        return Err(e);
                    }
                }
            }

            Ok(())
        }
    }

    fn set_models(
        &mut self,
        models: Vec<String>,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
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
                info!(
                    "Android target: storing model path for SDK-based loading: {}",
                    model_path
                );
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

                // Note: Do not call ensure_initialized() here; SDK will handle model loading.
                info!("Model path stored for Android SDK loading");
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
                    return Err(anyhow!(
                        "Android: Cannot start worker - model not loaded by SDK"
                    ));
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
        // Note: We do NOT clear cached_model here because:
        // 1. LlamaEngine is Clone, so multiple instances share the same Arc<Mutex<LlamaModel>>
        // 2. The global GLOBAL_ENGINE cache holds a reference to the engine
        // 3. Arc automatically manages reference counting and will free memory when last reference is dropped
        // 4. Clearing here would break the global cache and cause model to be freed prematurely
        if self.is_initialized {
            debug!(
                "LlamaEngine instance dropped (model remains in global cache if still referenced)"
            );
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
                let mut loading_model = self.current_loading_model.write().await;
                *loading_model = None;
            }

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
                    let mut loading_model = self.current_loading_model.write().await;
                    *loading_model = None;
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
                    let mut loading_model = self.current_loading_model.write().await;
                    *loading_model = None;
                }

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
        self.model_path
            .as_ref()
            .map(|path| (path.clone(), self.n_ctx, self.n_gpu_layers))
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
    pub async fn generate(
        &self,
        prompt: &str,
        max_tokens: usize,
    ) -> Result<(String, usize, usize)> {
        if !self.is_initialized {
            return Err(anyhow!("Llama.cpp engine is not initialized"));
        }

        debug!(
            "Generating text with prompt: {}, max_tokens: {}",
            prompt, max_tokens
        );

        // Use the real inference method with cached model
        self.generate_with_cached_model(prompt, max_tokens).await
    }

    /// Download a model from a URL
    pub async fn download_model(&self, url: &str, filename: &str) -> Result<PathBuf> {
        use futures_util::StreamExt;
        use reqwest::Client;
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
            return Err(anyhow!(
                "Failed to download model: HTTP {}",
                response.status()
            ));
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

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex as StdMutex;

    static ENV_LOCK: Lazy<StdMutex<()>> = Lazy::new(|| StdMutex::new(()));

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var(name).ok();
            std::env::set_var(name, value);
            Self { name, previous }
        }

        fn remove(name: &'static str) -> Self {
            let previous = std::env::var(name).ok();
            std::env::remove_var(name);
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.as_ref() {
                std::env::set_var(self.name, value);
            } else {
                std::env::remove_var(self.name);
            }
        }
    }

    fn set_mtime_seconds(path: &Path, secs: i64) {
        let time = filetime::FileTime::from_unix_time(secs, 0);
        filetime::set_file_mtime(path, time).unwrap();
    }

    #[test]
    fn session_state_cache_is_disabled_by_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _enabled = EnvVarGuard::remove(SESSION_STATE_CACHE_ENABLE_ENV);
        let _kv_enabled = EnvVarGuard::remove(SESSION_STATE_CACHE_ENABLE_KV_ENV);

        assert!(session_state_plan_for(
            "prompt",
            Some("session-secret"),
            Some("auto"),
            Some("/models/model.gguf"),
            2048,
            4096,
            99,
            &LlamaSplitModeArg::Layer,
            0,
            None,
        )
        .is_none());
    }

    #[test]
    fn session_state_plan_uses_hashed_paths_without_raw_inputs() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _enabled = EnvVarGuard::set(SESSION_STATE_CACHE_ENABLE_ENV, "1");
        let _kv_enabled = EnvVarGuard::remove(SESSION_STATE_CACHE_ENABLE_KV_ENV);
        let _dir = EnvVarGuard::set(
            SESSION_STATE_CACHE_DIR_ENV,
            temp.path().to_str().expect("utf8 temp path"),
        );

        let plan = session_state_plan_for(
            "very private prompt",
            Some("session-secret"),
            Some("auto"),
            Some("/models/model.gguf"),
            2048,
            4096,
            99,
            &LlamaSplitModeArg::Layer,
            0,
            Some("0,1"),
        )
        .expect("enabled cache plan");

        let rendered = plan.state_path.to_string_lossy();
        assert!(plan.state_path.starts_with(temp.path()));
        assert_eq!(plan.session_hash_short.len(), 12);
        assert!(!rendered.contains("session-secret"));
        assert!(!rendered.contains("very private prompt"));
        assert!(!rendered.contains("model.gguf"));
        assert_eq!(plan.model_key_hash.len(), 64);
        assert_eq!(plan.prompt_hash.len(), 64);
    }

    #[test]
    fn bypass_and_unknown_policy_do_not_select_real_state_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _enabled = EnvVarGuard::set(SESSION_STATE_CACHE_ENABLE_ENV, "1");
        let _dir = EnvVarGuard::set(
            SESSION_STATE_CACHE_DIR_ENV,
            temp.path().to_str().expect("utf8 temp path"),
        );

        for policy in ["bypass", "pin"] {
            let plan = session_state_plan_for(
                "prompt",
                Some("session-secret"),
                Some(policy),
                Some("/models/model.gguf"),
                2048,
                4096,
                99,
                &LlamaSplitModeArg::Layer,
                0,
                None,
            )
            .expect("policy plan");

            assert!(!plan.policy.should_load());
            assert!(!plan.policy.should_save());
            assert!(plan.state_path.ends_with("noop"));
        }
    }

    #[test]
    fn reset_clears_only_the_hashed_session_dir() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _enabled = EnvVarGuard::set(SESSION_STATE_CACHE_ENABLE_ENV, "1");
        let _dir = EnvVarGuard::set(
            SESSION_STATE_CACHE_DIR_ENV,
            temp.path().to_str().expect("utf8 temp path"),
        );

        let plan = session_state_plan_for(
            "prompt",
            Some("session-secret"),
            Some("reset"),
            Some("/models/model.gguf"),
            2048,
            4096,
            99,
            &LlamaSplitModeArg::Layer,
            0,
            None,
        )
        .expect("reset plan");
        std::fs::create_dir_all(&plan.session_dir).unwrap();
        std::fs::write(plan.session_dir.join("state.bin"), b"state").unwrap();
        std::fs::write(temp.path().join("neighbor.bin"), b"neighbor").unwrap();

        assert!(clear_session_state_dir(&plan.session_dir).unwrap());
        assert!(!plan.session_dir.exists());
        assert!(temp.path().join("neighbor.bin").exists());
    }

    #[test]
    fn checkpoint_meta_validates_state_hash_and_cache_identity() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _enabled = EnvVarGuard::set(SESSION_STATE_CACHE_ENABLE_ENV, "1");
        let _dir = EnvVarGuard::set(
            SESSION_STATE_CACHE_DIR_ENV,
            temp.path().to_str().expect("utf8 temp path"),
        );
        let plan = session_state_plan_for(
            "prompt",
            Some("session-secret"),
            Some("auto"),
            Some("/models/model.gguf"),
            2048,
            4096,
            99,
            &LlamaSplitModeArg::Layer,
            0,
            None,
        )
        .expect("cache plan");
        std::fs::create_dir_all(&plan.session_dir).unwrap();
        std::fs::write(&plan.state_path, b"state").unwrap();

        write_session_state_meta(&plan).unwrap();
        assert!(validate_session_state_meta(&plan).unwrap());

        std::fs::write(&plan.state_path, b"tampered").unwrap();
        assert!(!validate_session_state_meta(&plan).unwrap());

        std::fs::write(&plan.state_path, b"state").unwrap();
        let mut meta: SessionStateCheckpointMeta = serde_json::from_slice(
            &std::fs::read(session_state_meta_path(&plan.state_path)).unwrap(),
        )
        .unwrap();
        meta.version = SESSION_STATE_META_VERSION + 1;
        std::fs::write(
            session_state_meta_path(&plan.state_path),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        assert!(!validate_session_state_meta(&plan).unwrap());
    }

    #[test]
    fn missing_checkpoint_meta_is_not_valid() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _enabled = EnvVarGuard::set(SESSION_STATE_CACHE_ENABLE_ENV, "1");
        let _dir = EnvVarGuard::set(
            SESSION_STATE_CACHE_DIR_ENV,
            temp.path().to_str().expect("utf8 temp path"),
        );
        let plan = session_state_plan_for(
            "prompt",
            Some("session-secret"),
            Some("auto"),
            Some("/models/model.gguf"),
            2048,
            4096,
            99,
            &LlamaSplitModeArg::Layer,
            0,
            None,
        )
        .expect("cache plan");
        std::fs::create_dir_all(&plan.session_dir).unwrap();
        std::fs::write(&plan.state_path, b"state").unwrap();

        assert!(!validate_session_state_meta(&plan).unwrap());
    }

    #[test]
    fn state_checkpoint_quota_disabled_tracks_bytes_without_eviction() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _max = EnvVarGuard::remove(SESSION_STATE_CACHE_MAX_BYTES_ENV);
        std::fs::create_dir_all(temp.path().join("a")).unwrap();
        std::fs::write(temp.path().join("a").join("one.state"), vec![1u8; 10]).unwrap();
        std::fs::write(temp.path().join("a").join("ignore.bin"), vec![1u8; 100]).unwrap();

        let report = enforce_session_state_quota(temp.path()).unwrap();

        assert_eq!(report.bytes_current, 10);
        assert_eq!(report.evicted_files, 0);
        assert!(temp.path().join("a").join("one.state").exists());
        assert!(temp.path().join("a").join("ignore.bin").exists());
    }

    #[test]
    fn state_checkpoint_quota_evicts_oldest_state_files_only() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _metrics_guard = crate::handle::session_cache::worker_session_cache_test_guard();
        crate::handle::session_cache::reset_worker_session_cache_metrics_for_tests();
        let temp = tempfile::tempdir().unwrap();
        let _max = EnvVarGuard::set(SESSION_STATE_CACHE_MAX_BYTES_ENV, "15");
        let session_a = temp.path().join("a");
        let session_b = temp.path().join("b");
        std::fs::create_dir_all(&session_a).unwrap();
        std::fs::create_dir_all(&session_b).unwrap();
        let oldest = session_a.join("old.state");
        let newest = session_b.join("new.state");
        let ignored = session_b.join("keep.bin");
        std::fs::write(&oldest, vec![1u8; 10]).unwrap();
        std::fs::write(&newest, vec![2u8; 10]).unwrap();
        std::fs::write(&ignored, vec![3u8; 100]).unwrap();
        std::fs::write(session_state_meta_path(&oldest), b"old-meta").unwrap();
        std::fs::write(session_state_meta_path(&newest), b"new-meta").unwrap();
        set_mtime_seconds(&oldest, 10);
        set_mtime_seconds(&newest, 20);

        let report = enforce_session_state_quota(temp.path()).unwrap();

        assert_eq!(report.bytes_current, 10);
        assert_eq!(report.evicted_files, 1);
        assert!(!oldest.exists());
        assert!(!session_state_meta_path(&oldest).exists());
        assert!(newest.exists());
        assert!(session_state_meta_path(&newest).exists());
        assert!(ignored.exists());
        let metrics = crate::handle::session_cache::worker_session_cache_metrics_snapshot();
        assert_eq!(metrics.state_checkpoint_quota_eviction_total, 1);
        assert_eq!(metrics.state_checkpoint_bytes_current, 10);
        assert_eq!(metrics.state_checkpoint_max_bytes, 15);
    }

    #[test]
    fn blocking_stream_errors_are_forwarded_to_receiver() {
        let (tx, mut rx) = mpsc::channel::<Result<String>>(1);

        assert!(forward_blocking_stream_error(
            &tx,
            anyhow!("synthetic streaming failure")
        ));
        let err = rx
            .blocking_recv()
            .expect("stream error item")
            .expect_err("forwarded error");
        assert!(err.to_string().contains("synthetic streaming failure"));

        drop(rx);
        assert!(!forward_blocking_stream_error(
            &tx,
            anyhow!("receiver is gone")
        ));
    }
}
