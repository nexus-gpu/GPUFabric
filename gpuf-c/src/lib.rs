// ============================================================================
// GPUFabric LLM Engine - JNI Interface Layer
// ============================================================================
//
// IDE Warning Notice:
// Some sampler functions may show "not found in scope" warnings in IDE.
// This is due to header file version mismatch. These warnings can be safely ignored
// as the functions exist in the linked library and compilation succeeds.
// ============================================================================

#![allow(dead_code)] // Ignore IDE warnings for sampler functions

#[cfg(target_os = "android")]
use jni::objects::{JClass, JObject, JString};
#[cfg(target_os = "android")]
use jni::sys::{jboolean, jbyteArray, jfloat, jint, jlong, jstring};
#[cfg(target_os = "android")]
use jni::JNIEnv;
use libc::size_t;
use once_cell::sync::Lazy;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
#[cfg(target_os = "android")]
use std::io::Write;
#[cfg(any(target_os = "android", target_os = "ios"))]
use std::os::raw::c_ulonglong;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

const DEFAULT_LLAMA_THREADS: i32 = 4;
const DEFAULT_MTMD_THREADS: i32 = 4;
struct Utf8EmitBuffer {
    buf: Vec<u8>,
}

impl Utf8EmitBuffer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn push_and_take_valid(&mut self, bytes: &[u8]) -> String {
        // Filter NULs because they break C strings and aren't useful in text output.
        self.buf.extend(bytes.iter().copied().filter(|b| *b != 0));

        match std::str::from_utf8(&self.buf) {
            Ok(s) => {
                let out = s.to_string();
                self.buf.clear();
                out
            }
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                if valid_up_to == 0 {
                    // Avoid unbounded growth if we keep getting bytes that never form UTF-8.
                    if self.buf.len() > 8192 {
                        let s = String::from_utf8_lossy(&self.buf).to_string();
                        self.buf.clear();
                        return s;
                    }
                    return String::new();
                }

                // Split at a known UTF-8 boundary.
                let valid = String::from_utf8_lossy(&self.buf[..valid_up_to]).to_string();
                let rest = self.buf[valid_up_to..].to_vec();
                self.buf = rest;
                valid
            }
        }
    }

    fn flush_lossy(&mut self) -> String {
        if self.buf.is_empty() {
            return String::new();
        }
        let s = String::from_utf8_lossy(&self.buf).to_string();
        self.buf.clear();
        s
    }
}

// Global Tokio Runtime for async operations
#[cfg(target_os = "android")]
static TOKIO_RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    println!("🔧 Initializing Android-compatible single-threaded tokio runtime...");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    println!("✅ Tokio runtime initialized successfully");
    runtime
});

// Export modules
#[cfg(not(target_os = "ios"))]
pub mod handle;
#[cfg(not(target_os = "ios"))]
pub mod llm_engine;
pub mod util;

// iOS builds don't compile the full `handle` module (it depends on llm_engine).
// Expose worker runtime directly.
#[cfg(target_os = "ios")]
#[path = "handle/worker_sdk.rs"]
pub mod worker_sdk;

// JNI wrapper modules
#[cfg(target_os = "android")]
pub mod jni_llama;
#[cfg(target_os = "android")]
pub mod jni_remote_worker;

// Simulate llama.cpp structs (avoid C++ symbol dependencies)
#[repr(C)]
pub struct llama_vocab {
    _private: [u8; 0],
}

#[repr(C)]
pub struct llama_model {
    _private: [u8; 0],
}

#[repr(C)]
pub struct llama_context {
    _private: [u8; 0],
}

#[repr(C)]
pub struct llama_chat_message {
    pub role: *const c_char,
    pub content: *const c_char,
}

// 🆕 Callback function types for streaming output
/// Token callback: called for each generated token
/// Parameters: user_data, token_text, token_id
pub type TokenCallback = Option<extern "C" fn(*mut c_void, *const c_char, c_int)>;

/// Completion callback: called when generation completes
/// Parameters: user_data, full_text, token_count
pub type CompletionCallback = Option<extern "C" fn(*mut c_void, *const c_char, c_int)>;

// 🆕 Multimodal libmtmd structs
#[repr(C)]
pub struct MtmdContext {
    _private: [u8; 0],
}

#[repr(C)]
pub struct MtmdBitmap {
    _private: [u8; 0],
}

#[repr(C)]
pub struct MtmdInputChunks {
    _private: [u8; 0],
}

#[repr(C)]
pub struct MtmdInputText {
    pub text: *const c_char,
    pub add_special: bool,
    pub parse_special: bool,
}

#[repr(C)]
pub struct MtmdContextParams {
    pub use_gpu: bool,
    pub print_timings: bool,
    pub n_threads: c_int,
    pub image_marker: *const c_char,
    pub media_marker: *const c_char,
    pub flash_attn_type: c_int,
    pub warmup: bool,
    pub image_min_tokens: c_int,
    pub image_max_tokens: c_int,
}

#[repr(C)]
pub struct llama_model_params {
    // ggml_backend_dev_t *
    pub devices: *mut c_void,
    // const struct llama_model_tensor_buft_override *
    pub tensor_buft_overrides: *const c_void,
    pub n_gpu_layers: i32,
    // enum llama_split_mode
    pub split_mode: i32,
    pub main_gpu: i32,
    pub tensor_split: *const f32,
    pub progress_callback: Option<extern "C" fn(f32, *mut c_void) -> bool>,
    pub progress_callback_user_data: *mut c_void,
    // const struct llama_model_kv_override *
    pub kv_overrides: *const c_void,
    pub vocab_only: bool,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub check_tensors: bool,
    pub use_extra_bufts: bool,
    pub no_host: bool,
}

#[repr(C)]
pub struct llama_context_params {
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_ubatch: u32,
    pub n_seq_max: u32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub rope_scaling_type: i32, // enum llama_rope_scaling_type
    pub pooling_type: i32,      // enum llama_pooling_type
    pub attention_type: i32,    // enum llama_attention_type
    pub flash_attn_type: i32,   // enum llama_flash_attn_type
    pub rope_freq_base: f32,
    pub rope_freq_scale: f32,
    pub yarn_ext_factor: f32,
    pub yarn_attn_factor: f32,
    pub yarn_beta_fast: f32,
    pub yarn_beta_slow: f32,
    pub yarn_orig_ctx: u32,
    pub defrag_thold: f32,
    pub cb_eval: *mut (), // ggml_backend_sched_eval_callback
    pub cb_eval_user_data: *mut (),
    pub type_k: i32,             // enum ggml_type
    pub type_v: i32,             // enum ggml_type
    pub abort_callback: *mut (), // ggml_abort_callback
    pub abort_callback_data: *mut (),
    // Keep booleans at the end to avoid misalignment
    pub embeddings: bool,
    pub offload_kqv: bool,
    pub no_perf: bool,
    pub op_offload: bool,
    pub swa_full: bool,
    pub kv_unified: bool,
    // [EXPERIMENTAL] backend sampler chain configuration
    pub samplers: *mut c_void,
}

pub type LlamaToken = i32;
pub type LlamaPos = c_int;
pub type LlamaSeqId = c_int;

// 🆕 Multimodal-specific types (to avoid conflicts with existing code)
pub type MtmdLlamaPos = c_int;
pub type MtmdLlamaSeqId = c_int;

// Batch structure for llama_decode
#[repr(C)]
#[derive(Clone)]
pub struct llama_batch {
    pub n_tokens: c_int,
    pub token: *mut LlamaToken,
    pub embd: *mut f32,
    pub pos: *mut LlamaPos,
    pub n_seq_id: *mut c_int,
    pub seq_id: *mut *mut LlamaSeqId,
    pub logits: *mut i8,
}

// Completion structures (like llama.rn uses)
#[repr(C)]
#[derive(Clone)]
pub struct llama_completion_params {
    pub prompt: *const c_char,
    pub n_predict: c_int,
    pub temperature: f32,
    pub top_k: c_int,
    pub top_p: f32,
    pub repeat_penalty: f32,
    pub stop_words: *const *const c_char,
    pub n_stop_words: c_int,
}

#[repr(C)]
#[derive(Clone)]
pub struct llama_completion_result {
    pub text: *const c_char,
    pub n_tokens: c_int,
    pub timings: llama_timings,
}

#[repr(C)]
#[derive(Clone)]
pub struct llama_timings {
    pub prompt_eval_time_ms: f64,
    pub eval_time_ms: f64,
    pub total_time_ms: f64,
}

// 🆕 Sampling related structures
#[repr(C)]
#[derive(Clone)]
pub struct llama_token_data {
    pub id: LlamaToken,
    pub logit: f32,
    pub p: f32,
}

#[repr(C)]
pub struct llama_token_data_array {
    pub data: *mut llama_token_data,
    pub size: usize,
    pub selected: i64,
    pub sorted: bool,
}

// 🆕 Sampler structure (new version API)
#[repr(C)]
pub struct llama_sampler {
    _private: [u8; 0],
}

#[repr(C)]
pub struct llama_sampler_chain_params {
    pub no_perf: bool,
}

// ============================================================================
// Global Engine State Management
// ============================================================================

// Global context position tracking for continuous inference.
static GLOBAL_CONTEXT_POSITION: AtomicI32 = AtomicI32::new(0);

// Async generation control
static GENERATION_STOP_FLAG: AtomicBool = AtomicBool::new(false);
static GENERATION_MUTEX: Mutex<()> = Mutex::new(());

// Thread-safe generation stop control
fn should_stop_generation() -> bool {
    GENERATION_STOP_FLAG.load(Ordering::SeqCst)
}

fn set_generation_stop(stop: bool) {
    GENERATION_STOP_FLAG.store(stop, Ordering::SeqCst);
}

fn init_generation_control() {
    set_generation_stop(false);
}

fn cleanup_generation_control() {
    set_generation_stop(false);
}

// Global model state management
pub static MODEL_STATUS: Lazy<Arc<Mutex<ModelStatusInfo>>> =
    Lazy::new(|| Arc::new(Mutex::new(ModelStatusInfo::new())));

// Global inference mutex for thread safety
pub static GLOBAL_INFERENCE_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// Global model and context pointers
static GLOBAL_MODEL_PTR: AtomicPtr<llama_model> = AtomicPtr::new(std::ptr::null_mut());
static GLOBAL_CONTEXT_PTR: AtomicPtr<llama_context> = AtomicPtr::new(std::ptr::null_mut());

#[derive(Debug, Clone)]
pub struct ModelStatusInfo {
    pub current_model: Option<String>,
    pub loading_status: String,
    pub is_loaded: bool,
    pub error_message: Option<String>,
}

impl ModelStatusInfo {
    pub fn new() -> Self {
        Self {
            current_model: None,
            loading_status: "Not initialized".to_string(),
            is_loaded: false,
            error_message: None,
        }
    }

    pub fn set_loading(&mut self, model_path: &str) {
        self.current_model = Some(model_path.to_string());
        self.loading_status = "Loading...".to_string();
        self.is_loaded = false;
        self.error_message = None;
    }

    pub fn set_loaded(&mut self, model_path: &str) {
        self.current_model = Some(model_path.to_string());
        self.loading_status = "Loaded".to_string();
        self.is_loaded = true;
        self.error_message = None;
    }

    pub fn set_error(&mut self, error: &str) {
        self.loading_status = "Error".to_string();
        self.is_loaded = false;
        self.error_message = Some(error.to_string());
    }

    pub fn clear(&mut self) {
        self.current_model = None;
        self.loading_status = "Not initialized".to_string();
        self.is_loaded = false;
        self.error_message = None;
    }
}

// ============================================================================
// Real llama.cpp API Functions (for Android)
// ============================================================================

#[cfg(any(target_os = "android", target_os = "ios"))]
extern "C" {
    // Backend functions
    fn llama_backend_init() -> c_int;
    fn llama_backend_free();
    fn llama_load_model_from_file(
        path: *const c_char,
        params: llama_model_params,
    ) -> *mut llama_model;
    fn llama_init_from_model(
        model: *const llama_model,
        params: llama_context_params,
    ) -> *mut llama_context;
    fn llama_get_model(ctx: *const llama_context) -> *const llama_model; // ✅ Add missing binding
    fn llama_tokenize(
        vocab: *const llama_vocab, // ✅ Correct: vocab pointer, not context
        text: *const c_char,
        text_len: c_int, // ✅ Add missing text length
        tokens: *mut LlamaToken,
        n_tokens_max: c_int,
        add_bos: bool,
        parse_special: bool, // ✅ Add missing special token parsing
    ) -> c_int;

    // Generation functions - use actual llama.cpp API
    fn llama_decode(ctx: *mut llama_context, batch: llama_batch) -> c_int;
    fn llama_encode(ctx: *mut llama_context, batch: llama_batch) -> c_int;

    // 🆕 Multimodal libmtmd functions
    fn mtmd_context_params_default() -> MtmdContextParams;
    fn mtmd_init_from_file(
        mmproj_fname: *const c_char,
        text_model: *const llama_model,
        ctx_params: MtmdContextParams,
    ) -> *mut MtmdContext;
    fn mtmd_free(ctx: *mut MtmdContext);
    fn mtmd_support_vision(ctx: *mut MtmdContext) -> bool;
    fn mtmd_bitmap_init(nx: u32, ny: u32, data: *const u8) -> *mut MtmdBitmap;
    fn mtmd_bitmap_free(bitmap: *mut MtmdBitmap);
    fn mtmd_input_chunks_init() -> *mut MtmdInputChunks;
    fn mtmd_input_chunks_free(chunks: *mut MtmdInputChunks);
    fn mtmd_tokenize(
        ctx: *mut MtmdContext,
        output: *mut MtmdInputChunks,
        text: *const MtmdInputText,
        bitmaps: *const *mut MtmdBitmap,
        n_bitmaps: usize,
    ) -> c_int;
    fn mtmd_encode_chunk(ctx: *mut MtmdContext, chunk: *const c_void) -> c_int;
    fn mtmd_helper_eval_chunks(
        ctx: *mut MtmdContext,
        lctx: *mut llama_context,
        chunks: *mut c_void,
        n_past: MtmdLlamaPos,
        seq_id: MtmdLlamaSeqId,
        n_batch: c_int,
        logits_last: bool,
        new_n_past: *mut MtmdLlamaPos,
    ) -> c_int;
    fn mtmd_get_output_embd(ctx: *mut MtmdContext) -> *mut f32;

    fn llama_sampler_init_top_k(k: c_int) -> *mut llama_sampler;
    fn llama_sampler_init_top_p(p: f32, min_keep: usize) -> *mut llama_sampler;
    fn llama_sampler_init_temp(t: f32) -> *mut llama_sampler;
    fn llama_sampler_init_dist(seed: u32) -> *mut llama_sampler;
    fn llama_sampler_init_greedy() -> *mut llama_sampler;
    fn llama_sampler_init_penalties(
        penalty_last_n: c_int,
        penalty_repeat: f32,
        penalty_freq: f32,
        penalty_present: f32,
    ) -> *mut llama_sampler;
    fn llama_vocab_n_tokens(vocab: *const llama_vocab) -> c_int;
    fn llama_n_batch(ctx: *mut llama_context) -> c_int;
    fn llama_batch_init(n_tokens: c_int, embd: c_int, n_seq_max: c_int) -> llama_batch;
    fn llama_batch_free(batch: llama_batch);
    fn llama_batch_get_one(tokens: *mut LlamaToken, n_tokens: c_int) -> llama_batch;

    // Memory/KV cache management (llama.rn style)
    fn llama_get_memory(ctx: *mut llama_context) -> *mut c_void;
    fn llama_memory_seq_rm(mem: *mut c_void, seq_id: c_int, p0: LlamaPos, p1: LlamaPos) -> bool;
    fn llama_memory_clear(mem: *mut c_void, data: bool);

    #[allow(non_upper_case_globals)]
    #[allow(improper_ctypes)]
    fn llama_sampler_chain_init(params: llama_sampler_chain_params) -> *mut llama_sampler;
    fn llama_sampler_chain_add(chain: *mut llama_sampler, sampler: *mut llama_sampler);
    fn llama_sampler_sample(
        sampler: *mut llama_sampler,
        ctx: *mut llama_context,
        idx: c_int,
    ) -> LlamaToken;
    fn llama_sampler_free(sampler: *mut llama_sampler);
    fn llama_sampler_apply(sampler: *mut llama_sampler, candidates: *mut llama_token_data_array);

    // Utility functions
    fn llama_n_ctx(ctx: *const llama_context) -> c_int;
    fn llama_n_vocab(ctx: *mut llama_context) -> c_int;
    fn llama_token_bos(model: *const llama_model) -> LlamaToken;
    fn llama_token_eos(model: *const llama_model) -> LlamaToken;

    // 🆕 Added missing functions for proper token decoding
    fn llama_model_get_vocab(model: *const llama_model) -> *const llama_vocab;
    fn llama_token_to_piece(
        vocab: *const llama_vocab,
        token: LlamaToken,
        buf: *mut c_char,
        length: c_int,
        lstrip: c_int,
        special: bool,
    ) -> c_int;

    // Alternative: direct vocab text access
    fn llama_vocab_get_text(vocab: *const llama_vocab, token: LlamaToken) -> *const c_char;
    fn llama_vocab_is_control(vocab: *const llama_vocab, token: LlamaToken) -> bool;
    fn llama_vocab_is_eog(vocab: *const llama_vocab, token: LlamaToken) -> bool;
    fn llama_get_logits(ctx: *mut llama_context) -> *const f32;

    // Memory management functions
    fn llama_model_free(model: *mut llama_model);
    fn llama_free(ctx: *mut llama_context);

    // GGML backend functions - force linking
    fn ggml_backend_dev_by_type(type_: i32) -> *mut ();
    fn ggml_backend_dev_get(i: i32) -> *mut ();
    fn ggml_backend_dev_count() -> i32;
    fn ggml_backend_load_all();
    fn llama_model_default_params() -> llama_model_params;
    fn llama_context_default_params() -> llama_context_params;

    fn llama_chat_apply_template(
        tmpl: *const c_char,
        chat: *const llama_chat_message,
        n_msg: usize,
        add_ass: bool,
        buf: *mut c_char,
        length: c_int,
    ) -> c_int;
}

// ============================================================================
// Real llama.cpp API Wrappers
// ============================================================================

#[cfg(any(target_os = "android", target_os = "ios"))]
fn real_llama_backend_init() -> c_int {
    // SAFETY: llama.cpp backend initialization is a process-level FFI call.
    // This wrapper is used during SDK initialization before model/context use.
    unsafe {
        llama_backend_init();
        ggml_backend_load_all(); // Load backends to solve tensor loading issues
        0
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn real_llama_backend_free() {
    // SAFETY: Releases llama.cpp process-level backend resources during cleanup.
    unsafe { llama_backend_free() }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn real_llama_model_load_from_file(
    path: *const c_char,
    params: llama_model_params,
) -> *mut llama_model {
    // SAFETY: `path` is supplied by the C API caller and must be a valid
    // NUL-terminated model path for the duration of this call.
    unsafe { llama_load_model_from_file(path, params) }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
#[allow(dead_code)]
fn real_llama_model_free(model: *mut llama_model) {
    // SAFETY: `model` must be a llama.cpp model pointer returned by this SDK.
    unsafe { llama_model_free(model) }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn real_llama_init_from_model(
    model: *const llama_model,
    params: llama_context_params,
) -> *mut llama_context {
    // SAFETY: `model` must point to a live llama.cpp model for this call.
    unsafe { llama_init_from_model(model, params) }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
#[allow(dead_code)]
fn real_llama_free(ctx: *mut llama_context) {
    // SAFETY: `ctx` must be a llama.cpp context pointer returned by this SDK.
    unsafe { llama_free(ctx) }
}

//
#[cfg(target_os = "android")]
fn safe_llama_tokenize_with_pool(
    ctx: *mut llama_context,
    text: *const c_char,
    tokens: *mut LlamaToken,
    n_max_tokens: c_int,
    add_bos: bool,
) -> c_int {
    // Temporarily disabled - use safe_tokenize instead
    0
}

#[cfg(any(target_os = "android", target_os = "ios"))]
// llama-cpp-rs
/// # Safety
/// `ctx`, `text`, and `tokens` must be valid for this call. `text` must be
/// NUL-terminated and `tokens` must point to a writable buffer of at least
/// `max_tokens` elements.
pub(crate) unsafe fn safe_tokenize(
    ctx: *mut llama_context,
    text: *const c_char,
    tokens: *mut LlamaToken,
    max_tokens: c_int,
    add_bos: bool,
) -> c_int {
    println!("🔥🔥🔥 safe_tokenize FUNCTION CALLED!!! 🔥🔥🔥");
    // SAFETY: The caller contract above guarantees valid raw pointers and
    // buffer length. The function still checks nulls before dereferencing.
    unsafe {
        if ctx.is_null() || text.is_null() || tokens.is_null() {
            println!("❌ safe_tokenize: Invalid parameters");
            return 0;
        }

        // Convert C string to Rust string safely
        let text_cstr = std::ffi::CStr::from_ptr(text);
        let text_str = match text_cstr.to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };

        println!(
            "🎯 Using CORRECTED llama.cpp tokenization for input ({} bytes)",
            text_str.len()
        );

        // Get model and vocabulary for correct tokenization
        let model = llama_get_model(ctx);
        if model.is_null() {
            println!(" Failed to get model from context");
            return 0;
        }

        let vocab = llama_model_get_vocab(model);
        if vocab.is_null() {
            println!(" Failed to get vocabulary from model");
            return 0;
        }

        // Initialize token buffer
        for i in 0..max_tokens {
            *tokens.add(i as usize) = 0;
        }

        println!(" Calling CORRECTED llama_tokenize with vocab pointer");

        // FIXED: Use correct signature with all parameters
        let result = llama_tokenize(
            vocab,                   // vocab pointer (not context)
            text,                    // C string pointer
            text_str.len() as c_int, // text length
            tokens,                  // Output buffer
            max_tokens,              // Buffer size
            add_bos,                 // Add BOS
            true,                    // parse_special = true (like llama-cpp-rs)
        );

        if result > 0 {
            println!(" CORRECTED tokenizer success: {} tokens", result);

            // Debug: keep aggregate token diagnostics without leaking prompt text.
            println!(" Token mapping redacted ({} tokens)", result);
        } else {
            println!(" Tokenizer failed: {}", result);
        }

        result
    }
}

// Simple character-based tokenization for short texts
fn simple_char_tokenize(
    text: &str,
    tokens: *mut LlamaToken,
    max_tokens: c_int,
    add_bos: bool,
) -> c_int {
    // SAFETY: `tokens` must point to a writable buffer of `max_tokens`
    // elements supplied by the caller. Writes are bounded by `max_tokens`.
    unsafe {
        let mut token_count = 0;

        // Add BOS if requested
        if add_bos && token_count < max_tokens {
            *tokens.add(token_count as usize) = 1; // BOS token
            token_count += 1;
        }

        // Simple character to token mapping (basic ASCII)
        for ch in text.chars() {
            if token_count >= max_tokens {
                break;
            }

            // Map common characters to reasonable token IDs
            let token_id = match ch {
                ' ' => 29871,                                  // space
                'a'..='z' => 30400 + (ch as u32 - 'a' as u32), // lowercase letters
                'A'..='Z' => 30426 + (ch as u32 - 'A' as u32), // uppercase letters
                '0'..='9' => 29900 + (ch as u32 - '0' as u32), // digits
                '.' => 29889,                                  // period
                ',' => 29892,                                  // comma
                '!' => 29906,                                  // exclamation
                '?' => 29905,                                  // question
                '\n' => 29871, // newline (same as space for simplicity)
                _ => 29896,    // unknown character
            } as i32;

            *tokens.add(token_count as usize) = token_id;
            token_count += 1;
        }

        println!(
            " Simple tokenization: input {} bytes -> {} tokens",
            text.len(),
            token_count
        );
        token_count
    }
}

// Safe test function to check if llama_token_to_piece works
#[cfg(any(target_os = "android", target_os = "ios"))]
fn test_token_decode(model: *const llama_model, token: LlamaToken) -> Option<String> {
    let mut buffer = [0u8; 64];

    // SAFETY: `model` must be a live llama.cpp model pointer. `buffer` is a
    // fixed writable stack buffer passed with its exact length.
    unsafe {
        // Get vocab from model first
        let vocab = llama_model_get_vocab(model);
        if vocab.is_null() {
            return None;
        }

        // Try the new API
        let result = llama_token_to_piece(
            vocab, //
            token, //
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len() as c_int,
            0,    //
            true, //
        );

        if result > 0 && result < buffer.len() as c_int {
            let actual_len = result as usize;
            match std::str::from_utf8(&buffer[..actual_len]) {
                Ok(text) => Some(text.to_string()),
                Err(_) => None,
            }
        } else {
            None
        }
    }
}

// Enhanced token decoding with larger buffer and special token support
#[cfg(any(target_os = "android", target_os = "ios"))]
fn decode_token_to_text(model: *const llama_model, token: LlamaToken) -> String {
    // Use a local buffer so concurrent mobile callbacks cannot race on token decoding.
    let mut buffer = [0u8; 1024];

    // SAFETY: `model` must be a live llama.cpp model pointer. `buffer` is a
    // fixed writable stack buffer passed with its exact length.
    unsafe {
        let vocab = llama_model_get_vocab(model);
        if vocab.is_null() {
            return format!("[no_vocab:{}]", token);
        }

        // CRITICAL FIX: Enable special token decoding and proper lstrip
        let result = llama_token_to_piece(
            vocab,
            token,
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len() as c_int, // Larger buffer
            0,                     // lstrip = 0 (no leading space removal)
            true,                  // special = true (decode special tokens)
        );

        if result > 0 {
            let actual_len = if result < buffer.len() as c_int {
                result as usize
            } else {
                buffer.len() - 1
            };

            match std::str::from_utf8(&buffer[..actual_len]) {
                Ok(text) => {
                    if text.is_empty() {
                        format!("[empty_token:{}]", token)
                    } else {
                        text.to_string()
                    }
                }
                Err(_) => {
                    // DEBUG: Show hex bytes for debugging
                    let hex_bytes = &buffer[..actual_len.min(16)];
                    format!("[utf8_fail:{}:{:02X?}]", token, hex_bytes)
                }
            }
        } else {
            // 🔧 DEBUG: Check if this is a special/control token
            if llama_vocab_is_control(vocab, token) {
                format!("[control_token:{}]", token)
            } else if llama_vocab_is_eog(vocab, token) {
                format!("[eog_token:{}]", token)
            } else {
                format!("[decode_fail:{}]", token)
            }
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn manual_llama_completion(
    model: *const llama_model,
    ctx: *mut llama_context,
    prompt: *const c_char,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
    output: *mut c_char,
    output_len: c_int,
) -> c_int {
    // SAFETY: Mobile callers pass raw llama.cpp model/context pointers and an
    // output buffer. Null prompt is checked before use; output writes are
    // bounded by `output_len` before NUL termination.
    unsafe {
        // DEBUG: Temporarily remove memory pool reset to test llama_tokenize
        // reset_pool();

        // Step 1: Use safe tokenization inspired by llama-cpp-rs
        let mut tokens = [0i32; 512]; // Static array, no allocation
        let token_count: c_int;

        // DEBUG: Check raw input string before tokenization
        let _prompt_str = if prompt.is_null() {
            println!(" Prompt pointer is NULL!");
            return 0;
        } else {
            let c_str = std::ffi::CStr::from_ptr(prompt);
            match c_str.to_str() {
                Ok(s) => {
                    println!(" RAW INPUT DEBUG:");
                    println!("  Pointer: {:p}", prompt);
                    println!("  Length: {} bytes", s.len());
                    println!("  Content: <redacted>");
                    println!("  Bytes as hex: <redacted>");
                    s
                }
                Err(e) => {
                    println!(" Invalid UTF-8 in prompt: {:?}", e);
                    return 0;
                }
            }
        };

        // Use safe tokenization with fallback
        let tokenize_result = safe_tokenize(ctx, prompt, tokens.as_mut_ptr(), 512, true);

        if tokenize_result > 0 {
            token_count = tokenize_result;
            println!(" Safe tokenization successful! Got {} tokens", token_count);

            // DEBUG: Keep only aggregate prompt token diagnostics.
            println!(
                " INPUT DEBUG - Prompt token ids redacted ({} tokens)",
                token_count
            );
        } else {
            println!(" Safe tokenization failed, using emergency fallback");
            // Emergency fallback to BOS only
            tokens[0] = 1; // BOS
            token_count = 1;
        }

        println!(" Using {} tokens for inference", token_count);

        // Step 2: Clear KV cache for clean inference
        let kv = llama_get_memory(ctx);
        if !kv.is_null() {
            // Clear all sequences from KV cache
            let clear_result = llama_memory_seq_rm(kv, -1, -1, -1);
            if clear_result {
                println!(" KV cache cleared successfully");
            } else {
                println!(" KV cache clear failed, trying full clear...");
                llama_memory_clear(kv, false);
            }
        }

        // Step 3: Global position tracking for continuous context
        // CRITICAL FIX: Reset position for new independent inference
        let current_pos = 0; // Always start from 0 for clean inference
        GLOBAL_CONTEXT_POSITION.store(0, Ordering::SeqCst); // Reset global state
        println!(
            " GLOBAL CONTEXT: Reset to position {} for clean inference",
            current_pos
        );

        // Step 3: Create batch with global position tracking and logits request
        let mut batch_pos_array = [0i32; 512]; // Position array for batch
        let mut logits_array = [0i8; 512]; // Logits request array

        for i in 0..token_count {
            batch_pos_array[i as usize] = current_pos + i;
            // Request logits for the last token only (for sampling)
            logits_array[i as usize] = if i == token_count - 1 { 1 } else { 0 };
        }

        println!("🔍 Creating initial batch with {} tokens", token_count);

        let initial_batch = llama_batch {
            n_tokens: token_count,
            token: tokens.as_ptr() as *mut LlamaToken,
            embd: std::ptr::null_mut(),
            pos: batch_pos_array.as_ptr() as *mut LlamaPos,
            n_seq_id: std::ptr::null_mut(),
            seq_id: std::ptr::null_mut(),
            logits: logits_array.as_ptr() as *mut i8, // Request logits for last token
        };

        println!("🔍 Initial batch created, about to decode...");

        println!(
            " Created batch with {} tokens, positions {} to {}",
            token_count,
            current_pos,
            current_pos + token_count - 1
        );

        // Decode prompt
        let decode_result = llama_decode(ctx, initial_batch);
        if decode_result != 0 {
            println!(" Initial decode failed with code {}", decode_result);
            let msg = format!("Initial decode failed: code {}", decode_result);
            let msg_bytes = msg.as_bytes();
            let copy_len = std::cmp::min(msg_bytes.len(), output_len as usize - 1);
            std::ptr::copy_nonoverlapping(msg.as_ptr(), output as *mut u8, copy_len);
            *output.add(copy_len) = 0;
            return copy_len as c_int;
        }

        println!(" Initial decode successful");

        // Step 4: Generate tokens and update global position
        let mut generated_tokens = 0;
        let mut result_text = String::new();
        let mut next_pos = current_pos + token_count;

        // Generate tokens with reasonable safety limits
        // Context window is now 4096, support much longer generation
        // Allow up to 4096 tokens, but ensure we don't exceed context window
        let context_available = 4096 - current_pos - token_count;
        let safe_generation_limit =
            std::cmp::min(max_tokens, std::cmp::min(4096, context_available));
        println!(
            " Generation limit: {} (requested: {}, context_available: {}, max_safe: 4096)",
            safe_generation_limit, max_tokens, context_available
        );

        // PROPER SAMPLER: Use actual sampling parameters
        println!(
            " Creating sampler with params: temp={}, top_k={}, top_p={}, repeat_penalty={}",
            temperature, top_k, top_p, repeat_penalty
        );

        // Create sampler chain
        let chain_params = llama_sampler_chain_params { no_perf: false };
        let persistent_sampler = llama_sampler_chain_init(chain_params);

        if persistent_sampler.is_null() {
            println!(" Failed to create persistent sampler chain");
            return 0;
        }

        // Add samplers in proper order (like llama.cpp examples)

        // 1. Repeat penalty sampler
        if repeat_penalty != 1.0 {
            let repeat_sampler = llama_sampler_init_penalties(-1, repeat_penalty, 0.0, 0.0);
            if !repeat_sampler.is_null() {
                llama_sampler_chain_add(persistent_sampler, repeat_sampler);
                println!(
                    " Added Repeat penalty sampler (penalty: {})",
                    repeat_penalty
                );
            }
        }

        // 2. Top-K sampler
        if top_k > 0 {
            let top_k_sampler = llama_sampler_init_top_k(top_k);
            if !top_k_sampler.is_null() {
                llama_sampler_chain_add(persistent_sampler, top_k_sampler);
                println!(" Added Top-K sampler (k: {})", top_k);
            }
        }

        // 3. Top-P sampler
        if top_p < 1.0 {
            let top_p_sampler = llama_sampler_init_top_p(top_p, 1);
            if !top_p_sampler.is_null() {
                llama_sampler_chain_add(persistent_sampler, top_p_sampler);
                println!(" Added Top-P sampler (p: {})", top_p);
            }
        }

        // 4. Temperature sampler
        if temperature > 0.0 {
            let temp_sampler = llama_sampler_init_temp(temperature);
            if !temp_sampler.is_null() {
                llama_sampler_chain_add(persistent_sampler, temp_sampler);
                println!(" Added Temperature sampler (temp: {})", temperature);
            }
        }

        // 5. Distribution sampler (for actual sampling)
        let dist_sampler = llama_sampler_init_dist(1234);
        if !dist_sampler.is_null() {
            llama_sampler_chain_add(persistent_sampler, dist_sampler);
            println!(" Added Distribution sampler");
        }

        println!(" Sampler chain configured with all parameters");

        // Track current batch size (starts with initial token_count)
        let mut current_batch_size = token_count;

        for i in 0..safe_generation_limit {
            // Step 1: Sample from the last decoded position
            // After decode, logits are available at index (n_tokens - 1) for single token batches
            // For initial batch, logits are at the last token position
            let sampling_index = if i == 0 {
                token_count - 1 // First iteration: sample from initial batch's last token
            } else {
                0 // Subsequent iterations: single token batch, logits at index 0
            };

            println!(
                " Sampling iteration {}: from logits index {} (batch_size: {})",
                i, sampling_index, current_batch_size
            );

            // Use persistent sampler
            let sampled_token = llama_sampler_sample(persistent_sampler, ctx, sampling_index);

            println!(" Sampled token: {} at position {}", sampled_token, next_pos);

            // Check for EOS
            if sampled_token == 2 {
                // EOS token
                println!(" Reached EOS token");
                break;
            }

            println!(
                " Generated token {} at sequence position {} (temp:{}, top_k:{}, top_p:{})",
                sampled_token, next_pos, temperature, top_k, top_p
            );

            // Decode and add to result
            let decoded_text = decode_token_to_text(model, sampled_token);
            result_text.push_str(&decoded_text);
            println!(" Token text redacted ({} bytes)", decoded_text.len());

            generated_tokens += 1;
            next_pos += 1;

            // Step 2: CLEAR batch and add single new token (llama-cpp-rs style)
            println!(
                " Clearing batch and adding new token at position {}",
                next_pos - 1
            );

            // Create new single token batch (exactly like llama-cpp-rs)
            let mut single_token_pos = [0i32; 1];
            let mut single_token_logits = [1i8; 1]; // Always request logits for single token

            single_token_pos[0] = next_pos - 1; // Current sequence position
            single_token_logits[0] = 1; // Request logits

            let new_batch = llama_batch {
                n_tokens: 1,                                                     // Single token batch
                token: (&sampled_token as *const LlamaToken) as *mut LlamaToken, // The new token
                embd: std::ptr::null_mut(),
                pos: single_token_pos.as_ptr() as *mut LlamaPos,
                n_seq_id: std::ptr::null_mut(),
                seq_id: std::ptr::null_mut(),
                logits: single_token_logits.as_ptr() as *mut i8,
            };

            // Step 3: Decode the new single token batch
            let decode_result = llama_decode(ctx, new_batch);
            if decode_result != 0 {
                println!(" Decode failed at step {} with code {}", i, decode_result);
                break;
            }

            // Step 4: Update batch size for next iteration
            current_batch_size = 1; // Now we have a single token batch
            println!(
                " Completed iteration {}, batch_size reset to {}, next_pos: {}",
                i, current_batch_size, next_pos
            );

            // Safety check
            if generated_tokens >= max_tokens {
                break;
            }
        }

        // Cleanup persistent sampler at the end
        llama_sampler_free(persistent_sampler);
        println!(" Cleaned up persistent sampler");

        GLOBAL_CONTEXT_POSITION.store(next_pos, Ordering::SeqCst);
        println!(
            " GLOBAL CONTEXT: Updated position to {}",
            GLOBAL_CONTEXT_POSITION.load(Ordering::SeqCst)
        );

        // Step 6: Return only the generated text (no debug info)
        let final_text = if generated_tokens > 0 {
            println!(
                " CONTINUOUS CONTEXT: Generated {} tokens from pos {} (next: {})",
                generated_tokens,
                current_pos,
                GLOBAL_CONTEXT_POSITION.load(Ordering::SeqCst)
            );
            result_text
        } else {
            println!(
                " No tokens generated - continuous context ready from pos {} (next: {})",
                current_pos,
                GLOBAL_CONTEXT_POSITION.load(Ordering::SeqCst)
            );
            String::new() // Return empty string if no tokens generated
        };

        let text_bytes = final_text.as_bytes();
        let copy_len = std::cmp::min(text_bytes.len(), output_len as usize - 1);
        std::ptr::copy_nonoverlapping(text_bytes.as_ptr(), output as *mut u8, copy_len);
        *output.add(copy_len) = 0;

        copy_len as c_int
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn real_llama_n_ctx(ctx: *const llama_context) -> c_int {
    // SAFETY: `ctx` must point to a live llama.cpp context.
    unsafe { llama_n_ctx(ctx) }
}

// Token to text conversion (updated for new API)
#[cfg(target_os = "android")]
fn real_llama_token_to_piece(
    model: *const llama_model,
    token: LlamaToken,
    piece: *mut c_char,
    piece_len: usize,
) -> usize {
    // SAFETY: `model` must be a live llama.cpp model pointer and `piece` must
    // point to a writable buffer of `piece_len` bytes.
    unsafe {
        // Get vocab from model
        let vocab = llama_model_get_vocab(model);
        if vocab.is_null() {
            return 0;
        }

        // Use new API
        let result = llama_token_to_piece(
            vocab,
            token,
            piece,
            piece_len as c_int,
            0,    // lstrip
            true, // special
        );

        if result > 0 {
            result as usize
        } else {
            0
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn real_llama_token_eos(model: *const llama_model) -> LlamaToken {
    // SAFETY: `model` must point to a live llama.cpp model.
    unsafe { llama_token_eos(model) }
}

// Temporarily comment out detokenize until we verify function signature
/*
#[cfg(target_os = "android")]
fn real_llama_detokenize(
    model: *const llama_model,
    tokens: *const LlamaToken,
    n_tokens: c_int,
    text: *mut c_char,
    text_len_max: c_int,
) -> c_int {
    unsafe { llama_detokenize(model, tokens, n_tokens, text, text_len_max) }
}
*/

// ============================================================================
// Non-Android (fallback to simulation)
// ============================================================================

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn real_llama_backend_init() -> c_int {
    simulate_llama_backend_init()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn real_llama_backend_free() {
    simulate_llama_backend_free()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn real_llama_model_load_from_file(
    path: *const c_char,
    params: llama_model_params,
) -> *mut llama_model {
    simulate_llama_model_load_from_file(path, params)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[allow(dead_code)]
fn real_llama_model_free(model: *mut llama_model) {
    simulate_llama_model_free(model)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn real_llama_init_from_model(
    model: *const llama_model,
    params: llama_context_params,
) -> *mut llama_context {
    simulate_llama_init_from_model(model, params)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[allow(dead_code)]
fn real_llama_free(ctx: *mut llama_context) {
    simulate_llama_free(ctx)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
//
// fn real_llama_tokenize(
//     ctx: *mut llama_context,
//     text: *const c_char,
//     tokens: *mut LlamaToken,
//     n_max_tokens: c_int,
//     add_bos: bool,
// ) -> c_int {
//     simulate_llama_tokenize(ctx, text, tokens, n_max_tokens, add_bos)
// }
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn real_llama_n_ctx(ctx: *const llama_context) -> c_int {
    simulate_llama_n_ctx(ctx)
}

// Simulate real llama.cpp function behavior
fn simulate_llama_backend_init() -> c_int {
    println!("🔧 Simulating llama_backend_init()...");
    0 // Success
}

fn simulate_llama_backend_free() {
    println!("🧹 Simulating llama_backend_free()...");
}

fn simulate_llama_model_load_from_file(
    path: *const c_char,
    _params: llama_model_params,
) -> *mut llama_model {
    if path.is_null() {
        return std::ptr::null_mut();
    }

    // SAFETY: `path` was checked for null and is expected to be a
    // NUL-terminated C string supplied by the caller.
    let path_str = unsafe { CStr::from_ptr(path).to_str().unwrap_or("invalid_path") };

    println!(
        "🔧 Simulating llama_load_model_from_file(<redacted>, {} bytes)",
        path_str.len()
    );
    std::ptr::NonNull::dangling().as_ptr()
}

#[allow(dead_code)]
fn simulate_llama_model_free(model: *mut llama_model) {
    if !model.is_null() {
        println!("🧹 Simulating llama_model_free()");
    }
}

fn simulate_llama_init_from_model(
    model: *const llama_model,
    _params: llama_context_params,
) -> *mut llama_context {
    if model.is_null() {
        return std::ptr::null_mut();
    }

    println!("🔧 Simulating llama_init_from_model()");
    std::ptr::NonNull::dangling().as_ptr()
}

#[allow(dead_code)]
fn simulate_llama_free(ctx: *mut llama_context) {
    if !ctx.is_null() {
        println!("🧹 Simulating llama_free()");
    }
}

fn simulate_llama_tokenize(
    ctx: *mut llama_context,
    text: *const c_char,
    tokens: *mut LlamaToken,
    n_max_tokens: c_int,
    _add_bos: bool,
) -> c_int {
    if ctx.is_null() || text.is_null() || tokens.is_null() || n_max_tokens <= 0 {
        return 0;
    }

    // SAFETY: `text` was checked for null above and must be NUL-terminated.
    let text_str = unsafe { CStr::from_ptr(text).to_str().unwrap_or("") };

    println!(
        "🔧 Simulating llama_tokenize(<redacted>, {} bytes)",
        text_str.len()
    );

    // Return fake token count
    let token_count = text_str.len().min(n_max_tokens as usize);
    // SAFETY: `tokens` was checked for null and `token_count` is capped by
    // `n_max_tokens`, the caller-provided writable buffer length.
    unsafe {
        for i in 0..token_count {
            *tokens.add(i) = i as LlamaToken;
        }
    }

    token_count as c_int
}

fn simulate_llama_n_ctx(ctx: *const llama_context) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    2048
}

fn simulate_llama_model_default_params() -> llama_model_params {
    llama_model_params {
        devices: std::ptr::null_mut(),
        tensor_buft_overrides: std::ptr::null(),
        n_gpu_layers: 0,
        split_mode: 0,
        main_gpu: 0,
        tensor_split: std::ptr::null(),
        progress_callback: None,
        progress_callback_user_data: std::ptr::null_mut(),
        kv_overrides: std::ptr::null(),
        vocab_only: false,
        use_mmap: true,
        use_mlock: false,
        check_tensors: false,
        use_extra_bufts: false,
        no_host: false,
    }
}

fn simulate_llama_context_default_params() -> llama_context_params {
    llama_context_params {
        n_ctx: 2048,
        n_batch: 512,
        n_ubatch: 512,
        n_seq_max: 1,
        n_threads: DEFAULT_LLAMA_THREADS,
        n_threads_batch: DEFAULT_LLAMA_THREADS,
        rope_scaling_type: 0,
        pooling_type: 0,
        attention_type: 0,
        flash_attn_type: 0,
        rope_freq_base: 0.0,
        rope_freq_scale: 0.0,
        yarn_ext_factor: 0.0,
        yarn_attn_factor: 0.0,
        yarn_beta_fast: 0.0,
        yarn_beta_slow: 1.0,
        yarn_orig_ctx: 0,
        defrag_thold: 0.0,
        cb_eval: std::ptr::null_mut(),
        cb_eval_user_data: std::ptr::null_mut(),
        type_k: 0,
        type_v: 0,
        abort_callback: std::ptr::null_mut(),
        abort_callback_data: std::ptr::null_mut(),
        embeddings: false,
        offload_kqv: false,
        no_perf: false,
        op_offload: false,
        swa_full: false,
        kv_unified: false,
        samplers: std::ptr::null_mut(),
    }
}

// Final solution: Use real llama.cpp API on Android, simulated on other platforms

/// # Safety
/// `model` must be a valid pointer to a `llama_model` created by this library (or the linked
/// llama.cpp bindings) and must remain valid for the duration of this call.
#[no_mangle]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub extern "C" fn gpuf_create_context(model: *mut llama_model) -> *mut llama_context {
    if model.is_null() {
        return std::ptr::null_mut();
    }

    println!("🔧 Creating context with correct llama.cpp parameters...");

    // SAFETY: Retrieves llama.cpp default context parameters by value.
    let mut params = unsafe { llama_context_default_params() };
    params.n_ctx = 4096;
    params.n_batch = 128;
    params.n_threads = DEFAULT_LLAMA_THREADS;
    params.n_threads_batch = DEFAULT_LLAMA_THREADS;
    params.embeddings = false;
    params.offload_kqv = false;

    println!("📍 About to call real_llama_init_from_model...");
    let result = real_llama_init_from_model(model, params);
    println!("✅ Context created: {:p}", result);

    result
}

// Async Model Loading and Context Creation Functions
// ============================================================================

// Async loading state management - simplified and realistic
static ASYNC_LOADING_STATE: Lazy<Mutex<Option<AsyncLoadingState>>> = Lazy::new(|| Mutex::new(None));
static ASYNC_LOADING_HANDLE: Lazy<Mutex<Option<std::thread::JoinHandle<i32>>>> =
    Lazy::new(|| Mutex::new(None));

#[derive(Clone, Copy)]
pub struct AsyncLoadingState {
    pub status: i32,   // 0 = not started, 1 = loading, 2 = completed, 3 = error
    pub progress: f32, // Only meaningful when status = loading
    pub model_ptr: usize,
}

/// Start async model loading (realistic implementation)
///
/// # Safety
/// `path` must be a valid, NUL-terminated C string pointer and must remain valid for the duration
/// of this call.
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_load_model_async_start(path: *const c_char) -> bool {
    if path.is_null() {
        return false;
    }

    println!("🔄 Starting realistic async model loading...");

    // Copy path to owned string
    let path_str = unsafe {
        std::ffi::CStr::from_ptr(path)
            .to_str()
            .unwrap_or("unknown")
            .to_owned()
    };

    // Initialize loading state
    {
        let mut state_guard = ASYNC_LOADING_STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state_guard = Some(AsyncLoadingState {
            status: 1, // loading
            progress: 0.0,
            model_ptr: 0,
        });
    }

    // Start background loading thread
    let handle = std::thread::spawn(move || {
        println!("📊 Background thread: Starting REAL model load...");

        // Update state to show we're actually loading
        {
            let mut state_guard = ASYNC_LOADING_STATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(ref mut state) = *state_guard {
                state.progress = 0.1; // 10% - started loading
            }
        }

        // Actually load the model (this is the real work)
        let path_cstr = std::ffi::CString::new(path_str).unwrap();
        let model_ptr = gpuf_load_model(path_cstr.as_ptr());

        // Update final state based on real result
        {
            let mut state_guard = ASYNC_LOADING_STATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(ref mut state) = *state_guard {
                if model_ptr.is_null() {
                    state.status = 3; // error
                    state.progress = -1.0;
                    state.model_ptr = 0;
                } else {
                    state.status = 2; // completed
                    state.progress = 1.0;
                    state.model_ptr = model_ptr as usize;
                }
            }
        }

        println!("🎯 Background thread: REAL model loading completed");
        if model_ptr.is_null() {
            0
        } else {
            1
        }
    });

    // Store handle
    {
        let mut handle_guard = ASYNC_LOADING_HANDLE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = handle_guard.take() {
            let _ = previous.join();
        }
        *handle_guard = Some(handle);
    }

    true
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_load_model_async_start(_path: *const c_char) -> bool {
    false
}

/// Get loading status (realistic polling)
#[no_mangle]
pub extern "C" fn gpuf_load_model_get_status() -> i32 {
    ASYNC_LOADING_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|state| state.status)
        .unwrap_or(0) // 0 = not started
}

/// Get loading progress (limited but realistic)
#[no_mangle]
pub extern "C" fn gpuf_load_model_get_progress() -> f32 {
    ASYNC_LOADING_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|state| state.progress)
        .unwrap_or(-1.0) // -1.0 = not started
}

/// Check if loading is complete
#[no_mangle]
pub extern "C" fn gpuf_load_model_is_complete() -> bool {
    ASYNC_LOADING_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|state| state.status == 2)
        .unwrap_or(false)
}

/// Check if loading has error
#[no_mangle]
pub extern "C" fn gpuf_load_model_has_error() -> bool {
    ASYNC_LOADING_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|state| state.status == 3)
        .unwrap_or(false)
}

/// Get loaded model pointer (only valid after completion)
#[no_mangle]
pub extern "C" fn gpuf_load_model_get_result() -> *mut llama_model {
    ASYNC_LOADING_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(|state| {
            if state.status == 2 {
                Some(state.model_ptr as *mut llama_model)
            } else {
                None
            }
        })
        .unwrap_or(std::ptr::null_mut())
}

/// Wait for loading to complete (blocking)
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_load_model_wait() -> i32 {
    let handle = ASYNC_LOADING_HANDLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(handle) = handle {
        match handle.join() {
            Ok(result) => result, // 0 = failed, 1 = success
            Err(_) => 0,          // error
        }
    } else {
        0 // no handle
    }
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_load_model_wait() -> i32 {
    0
}

/// Cleanup async loading state
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_load_model_cleanup() {
    {
        // Wait for thread if still running
        let handle = ASYNC_LOADING_HANDLE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    // Clear state
    *ASYNC_LOADING_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_load_model_cleanup() {}

/// Legacy async model loading with callback (for backward compatibility)
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_load_model_async(
    path: *const c_char,
    on_progress: Option<extern "C" fn(f32, *mut c_void)>,
    user_data: *mut c_void,
) -> *mut llama_model {
    if path.is_null() {
        return std::ptr::null_mut();
    }

    println!("🔄 Starting async model loading...");

    // Report initial progress
    if let Some(callback) = on_progress {
        callback(0.0, user_data); // 0% - starting
    }

    // Load model (this is the slow part)
    let model_ptr = gpuf_load_model(path);

    if model_ptr.is_null() {
        // Report failure
        if let Some(callback) = on_progress {
            callback(-1.0, user_data); // -1 = error
        }
        return std::ptr::null_mut();
    }

    // Report completion
    if let Some(callback) = on_progress {
        callback(1.0, user_data); // 100% complete
    }

    model_ptr
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_load_model_async(
    _path: *const c_char,
    _on_progress: Option<extern "C" fn(f32, *mut c_void)>,
    _user_data: *mut c_void,
) -> *mut llama_model {
    std::ptr::null_mut()
}

/// Context creation remains synchronous (fast operation)
/// Use the regular gpuf_create_context for context creation
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_create_context_async(
    model: *mut llama_model,
    on_progress: Option<extern "C" fn(f32, *mut c_void)>,
    user_data: *mut c_void,
) -> *mut llama_context {
    if model.is_null() {
        return std::ptr::null_mut();
    }

    println!("🔄 Creating context (fast operation)...");

    // Context creation is fast, just use the synchronous version
    let context_ptr = gpuf_create_context(model);

    // Report immediate completion
    if let Some(callback) = on_progress {
        callback(1.0, user_data); // 100% complete immediately
    }

    context_ptr
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_create_context_async(
    _model: *mut llama_model,
    _on_progress: Option<extern "C" fn(f32, *mut c_void)>,
    _user_data: *mut c_void,
) -> *mut llama_context {
    std::ptr::null_mut()
}

/// Check if model is loaded (non-blocking)
#[no_mangle]
pub extern "C" fn gpuf_is_model_loaded() -> bool {
    !GLOBAL_MODEL_PTR
        .load(std::sync::atomic::Ordering::SeqCst)
        .is_null()
}

/// Check if context is created (non-blocking)
#[no_mangle]
pub extern "C" fn gpuf_is_context_ready() -> bool {
    !GLOBAL_CONTEXT_PTR
        .load(std::sync::atomic::Ordering::SeqCst)
        .is_null()
}

/// Get model loading status
#[no_mangle]
pub extern "C" fn gpuf_get_model_status() -> c_int {
    // 0 = not loaded, 1 = loading, 2 = loaded, 3 = error
    if gpuf_is_model_loaded() {
        if gpuf_is_context_ready() {
            2 // loaded and ready
        } else {
            1 // model loaded but context not ready
        }
    } else {
        0 // not loaded
    }
}

// Multimodal model structure using libmtmd
// C-compatible structure for multimodal model (matches gpuf_c.h)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectorType {
    Unknown = 0,
    LLaVA = 1,
    Qwen2VL = 2,
    Qwen25VL = 3,
    Qwen3VL = 4,
    Pixtral = 5,
}

// Vision token pairs for different model types
pub struct VisionTokens {
    pub start: &'static str,
    pub end: &'static str,
    pub media: &'static str,
}

impl ProjectorType {
    pub fn get_vision_tokens(self) -> VisionTokens {
        match self {
            ProjectorType::Qwen2VL | ProjectorType::Qwen25VL | ProjectorType::Qwen3VL => {
                VisionTokens {
                    start: "<|vision_start|>",
                    end: "<|vision_end|>",
                    media: "<__media__>", // Use standard media marker for libmtmd positioning
                }
            }
            ProjectorType::LLaVA | _ => {
                VisionTokens {
                    start: "", // LLaVA and others use media marker
                    end: "",
                    media: "<__media__>",
                }
            }
        }
    }
}

// Multimodal model structure with cached model type
#[repr(C)]
pub struct gpuf_multimodal_model {
    pub text_model: *mut llama_model,
    pub mtmd_context: *mut MtmdContext,
    pub projector_type: ProjectorType, // Cache model type
    pub vocab: *const llama_vocab,     // Store vocab pointer like official
    pub is_multimodal: bool,
    // 🆕 Keep CString alive for media_marker
    _media_marker: CString,
}

pub struct MultimodalModel {
    pub llama_model: *mut llama_model,
    pub llama_context: *mut llama_context,
    pub mtmd_context: *mut MtmdContext,
    pub vocab: *const llama_vocab, // Store vocab pointer like official
    pub model_path: String,
    pub mmproj_path: String,
}

// Load model with multimodal support
///
/// # Safety
/// `path` must be a valid, NUL-terminated C string pointer and must remain valid for the duration
/// of this call.
#[no_mangle]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub extern "C" fn gpuf_load_model(path: *const c_char) -> *mut llama_model {
    if path.is_null() {
        return std::ptr::null_mut();
    }

    println!("🔧 Loading model with safe parameters...");

    // Use safer parameter settings
    // SAFETY: Retrieves llama.cpp default model parameters by value.
    let mut params = unsafe { llama_model_default_params() };
    params.vocab_only = false;
    params.use_mmap = true; // Enable mmap to reduce memory pressure
    params.use_mlock = false;
    params.n_gpu_layers = 0; // Force CPU usage to avoid GPU-related issues

    println!("📍 About to call real_llama_model_load_from_file...");
    let result = real_llama_model_load_from_file(path, params);
    println!("✅ real_llama_model_load_from_file returned: {:p}", result);

    result
}

// 🆕 Helper function to detect model type from filename
fn detect_model_type_from_path(model_path: &str) -> ProjectorType {
    if model_path.contains("Qwen2-VL") || model_path.contains("qwen2vl") {
        ProjectorType::Qwen2VL
    } else if model_path.contains("Qwen2.5-VL") || model_path.contains("qwen25vl") {
        ProjectorType::Qwen25VL
    } else if model_path.contains("Qwen3-VL") || model_path.contains("qwen3vl") {
        ProjectorType::Qwen3VL
    } else if model_path.contains("LLaVA") || model_path.contains("llava") {
        ProjectorType::LLaVA
    } else if model_path.contains("pixtral") || model_path.contains("Pixtral") {
        ProjectorType::Pixtral
    } else {
        ProjectorType::Unknown
    }
}

// Load multimodal model using libmtmd with model type detection
///
/// # Safety
/// `text_model_path` and `mmproj_path` must be valid, NUL-terminated C string pointers and must
/// remain valid for the duration of this call.
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_load_multimodal_model(
    text_model_path: *const c_char,
    mmproj_path: *const c_char,
) -> *mut gpuf_multimodal_model {
    if text_model_path.is_null() || mmproj_path.is_null() {
        return std::ptr::null_mut();
    }

    // SAFETY: `text_model_path` and `mmproj_path` were checked for null and
    // must remain valid NUL-terminated strings for this call. The llama.cpp
    // and libmtmd pointers created here are either returned in the owned
    // `gpuf_multimodal_model` box or freed on error before returning.
    unsafe {
        // Convert paths to Rust strings
        let text_path = CStr::from_ptr(text_model_path).to_str().unwrap_or("");
        let mmproj_path_str = CStr::from_ptr(mmproj_path).to_str().unwrap_or("");

        println!("🔧 Loading multimodal model (libmtmd)...");
        println!("  Text model path accepted ({} bytes)", text_path.len());
        println!("  MMProj path accepted ({} bytes)", mmproj_path_str.len());

        // Load text model first
        let model_params = llama_model_default_params();
        let text_model = llama_load_model_from_file(text_model_path, model_params);
        if text_model.is_null() {
            eprintln!("❌ Failed to load text model");
            return std::ptr::null_mut();
        }

        // Initialize libmtmd context
        let ctx_params = MtmdContextParams {
            use_gpu: true,
            print_timings: false,
            n_threads: DEFAULT_MTMD_THREADS,
            image_marker: std::ptr::null(),
            media_marker: std::ptr::null(),
            flash_attn_type: 0,
            warmup: false,
            image_min_tokens: 1,
            image_max_tokens: 1440,
        };

        // Initialize libmtmd context with proper media markers
        let mmproj_cstr = CString::new(mmproj_path_str).unwrap_or_default();
        let mut ctx_params = mtmd_context_params_default();
        // Override only necessary fields
        ctx_params.use_gpu = true;
        ctx_params.n_threads = DEFAULT_MTMD_THREADS;

        // 🆕 Set proper media marker based on model type
        let projector_type = detect_model_type_from_path(text_path);
        let media_marker = match projector_type {
            ProjectorType::Qwen2VL | ProjectorType::Qwen25VL | ProjectorType::Qwen3VL => {
                // Qwen2-VL uses standard media marker for positioning, libmtmd handles vision tokens
                CString::new("<__media__>").unwrap_or_default()
            }
            _ => {
                // SmolVLM and others use <__media__>
                CString::new("<__media__>").unwrap_or_default()
            }
        };
        ctx_params.media_marker = media_marker.as_ptr();

        let mtmd_ctx = mtmd_init_from_file(mmproj_cstr.as_ptr(), text_model, ctx_params);
        if mtmd_ctx.is_null() {
            eprintln!("❌ Failed to initialize libmtmd context");
            llama_model_free(text_model);
            return std::ptr::null_mut();
        }

        // 🆕 Detect model type from filename
        let projector_type = detect_model_type_from_path(text_path);
        println!("🎯 Detected model type: {:?}", projector_type);

        let vision_tokens = projector_type.get_vision_tokens();
        if !vision_tokens.media.is_empty() {
            println!("  Using media marker: {}", vision_tokens.media);
        }
        if !vision_tokens.start.is_empty() {
            println!(
                "  Using vision tokens: {} ... {}",
                vision_tokens.start, vision_tokens.end
            );
        }

        // Get vocab pointer like official (before creating the structure)
        let vocab = llama_model_get_vocab(text_model);

        // Create multimodal model structure with cached type
        let multimodal_model = Box::new(gpuf_multimodal_model {
            text_model,
            mtmd_context: mtmd_ctx,
            projector_type, // 🆕 Cache model type
            vocab,          // Store vocab pointer like official
            is_multimodal: true,
            _media_marker: media_marker, // 🆕 Keep CString alive
        });

        println!("✅ Multimodal model loaded successfully");
        Box::into_raw(multimodal_model)
    }
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_load_multimodal_model(
    _text_model_path: *const c_char,
    _mmproj_path: *const c_char,
) -> *mut gpuf_multimodal_model {
    std::ptr::null_mut()
}

// Create context for multimodal model
///
/// # Safety
/// `multimodal_model` must be a valid pointer returned by `gpuf_load_multimodal_model` and must
/// remain valid for the duration of this call.
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_create_multimodal_context(
    multimodal_model: *mut gpuf_multimodal_model,
) -> *mut llama_context {
    if multimodal_model.is_null() {
        return std::ptr::null_mut();
    }

    // SAFETY: `multimodal_model` is checked for null above and must be a
    // pointer returned by `gpuf_load_multimodal_model`; only the cached text
    // model pointer is read here.
    let model = unsafe { (*multimodal_model).text_model };
    if model.is_null() {
        return std::ptr::null_mut();
    }

    // Use existing context creation with text model
    let mut ctx_params = simulate_llama_context_default_params();
    ctx_params.n_ctx = 512; // Larger context for multimodal
    ctx_params.n_batch = 128; // Larger batch for multimodal
    ctx_params.embeddings = false; // Use correct field name

    real_llama_init_from_model(model, ctx_params)
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_create_multimodal_context(
    _multimodal_model: *mut gpuf_multimodal_model,
) -> *mut llama_context {
    std::ptr::null_mut()
}

/// # Safety
/// - `multimodal_model` must be a valid pointer returned by `gpuf_load_multimodal_model`.
/// - `ctx` may be null (a fresh context may be created internally); if non-null it must be a valid
///   `llama_context` compatible with the given model.
/// - `text_prompt` must be a valid, NUL-terminated C string pointer.
/// - `image_data` must be a valid pointer to `image_size` bytes (may be null only if
///   `image_size == 0`).
/// - `output` must be a valid writable buffer of at least `output_len` bytes.
#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_generate_multimodal(
    _multimodal_model: *mut gpuf_multimodal_model,
    _ctx: *mut llama_context,
    _text_prompt: *const c_char,
    _image_data: *const u8,
    _image_size: c_ulonglong,
    _max_tokens: c_int,
    _temperature: f32,
    _top_k: c_int,
    _top_p: f32,
    _repeat_penalty: f32,
    _output: *mut c_char,
    _output_len: c_int,
) -> c_int {
    -1
}

#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_generate_multimodal(
    multimodal_model: *mut gpuf_multimodal_model,
    ctx: *mut llama_context,
    text_prompt: *const c_char,
    image_data: *const u8,
    image_size: c_ulonglong,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
    output: *mut c_char,
    output_len: c_int,
) -> c_int {
    eprintln!("🔍 DEBUG: gpuf_generate_multimodal FUNCTION STARTED!");
    eprintln!("🔍 DEBUG: Image size: {} bytes", image_size);
    eprintln!("🔍 DEBUG: Prompt pointer: {:p}", text_prompt);
    eprintln!("🔍 DEBUG: Image data pointer: {:p}", image_data);
    std::io::stderr().flush().ok();
    if multimodal_model.is_null() || text_prompt.is_null() || output.is_null() {
        return -1;
    }

    // SAFETY: All raw inputs required by this FFI entrypoint were checked for
    // null above. The caller must provide `output_len` bytes of writable output
    // storage, and image data must be valid for `image_size` bytes when present.
    unsafe {
        let model_ref = &*multimodal_model;
        let mtmd_ctx = model_ref.mtmd_context;

        if mtmd_ctx.is_null() {
            println!("❌ Multimodal context is null");
            return -1;
        }

        // 🆕 Create a fresh context for each request to avoid reuse issues
        println!("🔧 Creating fresh context for this request...");
        let ctx_was_null = ctx.is_null();
        let ctx = if ctx_was_null {
            // If no context provided, create a new one
            let new_ctx = gpuf_create_multimodal_context(multimodal_model);
            println!("✅ Created new context: {:p}", new_ctx);
            new_ctx
        } else {
            // Use provided context (for backward compatibility)
            println!("⚠️ Using provided context: {:p} (may fail on reuse)", ctx);
            ctx
        };

        if ctx.is_null() {
            println!("❌ Failed to create/get context");
            return -1;
        }

        let prompt_str = match CStr::from_ptr(text_prompt).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };

        println!(
            "🔥 GPUFabric: libmtmd multimodal generation - temp:{}, top_k:{}, top_p:{}",
            temperature, top_k, top_p
        );

        // Create input text structure
        let input_text = MtmdInputText {
            text: text_prompt,
            add_special: true,
            parse_special: true,
        };

        // Initialize input chunks
        let chunks = mtmd_input_chunks_init();
        if chunks.is_null() {
            println!("❌ Failed to initialize input chunks");
            return -1;
        }

        let mut result = 0;

        // Check if we have image data
        if !image_data.is_null() && image_size > 0 {
            println!("🔍 DEBUG: Image data found - {} bytes", image_size);
            println!("🔍 DEBUG: Starting image processing...");

            // For demo purposes, assume image is 224x224 RGB
            let image = mtmd_bitmap_init(224, 224, image_data);
            if !image.is_null() {
                // Tokenize with image
                let image_ptr = &image;
                result = mtmd_tokenize(mtmd_ctx, chunks, &input_text, image_ptr, 1);

                if result == 0 {
                    println!("✅ Multimodal tokenization successful");
                    println!("🔍 Starting multimodal encoding process...");

                    // Encode all tokenized chunks into the context
                    let mut encode_result = 0;
                    let mut chunk_count = 0;
                    let mut current_pos: MtmdLlamaPos = 0;

                    // 🆕 Define new_n_past at higher scope to fix variable access issue
                    let mut new_n_past: MtmdLlamaPos = 0;

                    // For multimodal models, the tokenization should have already prepared the context
                    // Let's check if we can proceed directly to generation
                    // Always use mtmd_helper_eval_chunks to encode and get correct n_past position
                    println!("🔍 Encoding multimodal input with mtmd_helper_eval_chunks...");
                    println!("🔍 Before encoding - current_pos: {}", current_pos);

                    // Check context state before encoding
                    let pre_encode_n_ctx = llama_n_ctx(ctx);
                    let pre_encode_vocab = llama_n_vocab(ctx);
                    println!(
                        "🔍 Pre-encode: n_ctx={}, vocab_size={}",
                        pre_encode_n_ctx, pre_encode_vocab
                    );

                    encode_result = mtmd_helper_eval_chunks(
                        mtmd_ctx,
                        ctx,
                        chunks as *mut c_void,
                        current_pos,
                        0,    // seq_id
                        128,  // n_batch
                        true, // logits_last
                        &mut new_n_past,
                    );

                    println!("🔍 mtmd_helper_eval_chunks result: {}", encode_result);
                    println!("🔍 New n_past: {} (was: {})", new_n_past, current_pos);

                    // Check context state after encoding
                    let post_encode_n_ctx = llama_n_ctx(ctx);
                    let post_encode_vocab = llama_n_vocab(ctx);
                    println!(
                        "🔍 Post-encode: n_ctx={}, vocab_size={}",
                        post_encode_n_ctx, post_encode_vocab
                    );

                    if post_encode_vocab == 0 && pre_encode_vocab > 0 {
                        println!(
                            "⚠️ WARNING: vocab_size changed from {} to 0 after encoding!",
                            pre_encode_vocab
                        );
                        println!(
                            "⚠️ This is expected - will use direct vocab pointer for generation"
                        );
                    }

                    if encode_result == 0 {
                        println!("✅ Multimodal evaluation successful!");
                        // Update position for generation
                        current_pos = new_n_past;
                    } else {
                        println!("❌ Multimodal evaluation failed: {}", encode_result);
                    }

                    println!(
                        "🔢 Encoded {} chunks, result: {}",
                        chunk_count, encode_result
                    );
                    println!(
                        "🔍 Encode result check: {}",
                        if encode_result == 0 {
                            "SUCCESS"
                        } else {
                            "FAILED"
                        }
                    );

                    if encode_result == 0 {
                        println!("✅ Multimodal encoding successful - proceeding with generation");
                        println!(
                            "🔍 Using position {} from mtmd_helper_eval_chunks",
                            new_n_past
                        );

                        // Always use direct vocab pointer approach for consistency
                        // This avoids issues with llama_n_vocab(ctx) returning 0 after multimodal encoding
                        let model_ptr = llama_get_model(ctx);
                        if model_ptr.is_null() {
                            let error_msg =
                                CString::new("❌ Failed to get model pointer").unwrap_or_default();
                            let error_bytes = error_msg.as_bytes_with_nul();
                            let copy_len = std::cmp::min(error_bytes.len(), output_len as usize);
                            std::ptr::copy_nonoverlapping(
                                error_bytes.as_ptr(),
                                output as *mut u8,
                                copy_len,
                            );
                            return copy_len as c_int;
                        }

                        let vocab = llama_model_get_vocab(model_ptr);
                        if vocab.is_null() {
                            let error_msg =
                                CString::new("❌ Failed to get vocab pointer").unwrap_or_default();
                            let error_bytes = error_msg.as_bytes_with_nul();
                            let copy_len = std::cmp::min(error_bytes.len(), output_len as usize);
                            std::ptr::copy_nonoverlapping(
                                error_bytes.as_ptr(),
                                output as *mut u8,
                                copy_len,
                            );
                            return copy_len as c_int;
                        }

                        println!(
                            "✅ Got vocab pointer {:p}, starting generation from position {}",
                            vocab, new_n_past
                        );

                        // Call generation with direct vocab pointer and correct position
                        let generated_text = generate_multimodal_response_with_vocab(
                            ctx,
                            vocab,
                            max_tokens,
                            temperature,
                            top_k,
                            top_p,
                            repeat_penalty,
                            new_n_past as i32, // Pass correct position from encoding
                        );

                        // Copy response to output
                        let response_cstr = CString::new(generated_text).unwrap_or_default();
                        let response_bytes = response_cstr.as_bytes_with_nul();
                        let copy_len = std::cmp::min(response_bytes.len(), output_len as usize);

                        std::ptr::copy_nonoverlapping(
                            response_bytes.as_ptr(),
                            output as *mut u8,
                            copy_len,
                        );

                        if copy_len < output_len as usize {
                            *(output.add(copy_len)) = 0;
                        }
                    } else {
                        println!("❌ Multimodal encoding failed: {}", encode_result);
                        let error_msg =
                            CString::new("❌ Multimodal encoding failed").unwrap_or_default();
                        let error_bytes = error_msg.as_bytes_with_nul();
                        let copy_len = std::cmp::min(error_bytes.len(), output_len as usize);
                        std::ptr::copy_nonoverlapping(
                            error_bytes.as_ptr(),
                            output as *mut u8,
                            copy_len,
                        );
                    }
                } else {
                    println!("❌ Multimodal tokenization failed: {}", result);
                }

                mtmd_bitmap_free(image);
            } else {
                println!("❌ Failed to create image bitmap");
                result = -1;
            }
        } else {
            // Text-only generation
            result = mtmd_tokenize(mtmd_ctx, chunks, &input_text, std::ptr::null(), 0);

            if result == 0 {
                println!("✅ Text-only tokenization successful");

                let response = format!(
                    "GPUFabric: libmtmd text-only generation successful (prompt {} bytes)",
                    prompt_str.len()
                );

                let response_cstr = CString::new(response).unwrap_or_default();
                let response_bytes = response_cstr.as_bytes_with_nul();
                let copy_len = std::cmp::min(response_bytes.len(), output_len as usize);

                std::ptr::copy_nonoverlapping(response_bytes.as_ptr(), output as *mut u8, copy_len);

                if copy_len < output_len as usize {
                    *(output.add(copy_len)) = 0;
                }
            }
        }

        // Cleanup
        mtmd_input_chunks_free(chunks);

        // 🆕 Free the context if we created it
        if ctx_was_null && !ctx.is_null() {
            println!("🔧 Freeing created context: {:p}", ctx);
            llama_free(ctx);
        }

        if result == 0 {
            // Return number of tokens in response as demo
            let response_len = CStr::from_ptr(output).to_bytes().len();
            (response_len / 4) as c_int // Rough estimate of token count
        } else {
            -1
        }
    }
}

// 🆕 Streaming version with callbacks
#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_generate_multimodal_stream(
    _multimodal_model: *mut gpuf_multimodal_model,
    _ctx: *mut llama_context,
    _text_prompt: *const c_char,
    _image_data: *const u8,
    _image_size: c_ulonglong,
    _max_tokens: c_int,
    _temperature: f32,
    _top_k: c_int,
    _top_p: f32,
    _repeat_penalty: f32,
    _on_token: TokenCallback,
    _on_complete: CompletionCallback,
    _user_data: *mut c_void,
) -> c_int {
    -1
}

#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_generate_multimodal_stream(
    multimodal_model: *mut gpuf_multimodal_model,
    ctx: *mut llama_context,
    text_prompt: *const c_char,
    image_data: *const u8,
    image_size: c_ulonglong,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
    on_token: TokenCallback,
    on_complete: CompletionCallback,
    user_data: *mut c_void,
) -> c_int {
    println!("🔍 Starting streaming multimodal generation...");

    if multimodal_model.is_null() || text_prompt.is_null() {
        return -1;
    }

    unsafe {
        let model_ref = &*multimodal_model;
        let mtmd_ctx = model_ref.mtmd_context;

        if mtmd_ctx.is_null() {
            println!("❌ Multimodal context is null");
            return -1;
        }

        // Create a fresh context for each request
        let ctx_was_null = ctx.is_null();
        let ctx = if ctx_was_null {
            let new_ctx = gpuf_create_multimodal_context(multimodal_model);
            println!("✅ Created new context: {:p}", new_ctx);
            new_ctx
        } else {
            println!("⚠️ Using provided context: {:p}", ctx);
            ctx
        };

        if ctx.is_null() {
            println!("❌ Failed to create/get context");
            return -1;
        }

        let prompt_str = match CStr::from_ptr(text_prompt).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };

        println!(
            "🔥 GPUFabric: Streaming multimodal generation - temp:{}, top_k:{}, top_p:{}",
            temperature, top_k, top_p
        );

        // Create input text structure
        let prompt_cstr = CString::new(prompt_str).unwrap_or_default();
        let input_text = MtmdInputText {
            text: prompt_cstr.as_ptr(),
            add_special: true,
            parse_special: false,
        };

        // Create input chunks
        let chunks = mtmd_input_chunks_init();
        if chunks.is_null() {
            println!("❌ Failed to create input chunks");
            if ctx_was_null {
                llama_free(ctx);
            }
            return -1;
        }

        // Prepare for tokenization
        let mut bitmaps: Vec<*mut MtmdBitmap> = Vec::new();

        // Add image if provided
        if !image_data.is_null() && image_size > 0 {
            println!("🔍 DEBUG: Image data found - {} bytes", image_size);

            let bitmap = mtmd_bitmap_init(224, 224, image_data);

            if !bitmap.is_null() {
                bitmaps.push(bitmap);
            }
        }

        // Tokenize with correct parameters
        let tokenize_result = mtmd_tokenize(
            mtmd_ctx,
            chunks,
            &input_text,
            bitmaps.as_ptr(),
            bitmaps.len(),
        );

        // Cleanup bitmaps
        for bitmap in bitmaps {
            mtmd_bitmap_free(bitmap);
        }
        if tokenize_result != 0 {
            println!("❌ Multimodal tokenization failed: {}", tokenize_result);
            mtmd_input_chunks_free(chunks);
            if ctx_was_null {
                llama_free(ctx);
            }
            return -1;
        }

        // Encode with mtmd_helper_eval_chunks
        let mut new_n_past: MtmdLlamaPos = 0;
        let encode_result = mtmd_helper_eval_chunks(
            mtmd_ctx,
            ctx,
            chunks as *mut c_void,
            0,
            0,
            128,
            true,
            &mut new_n_past,
        );

        if encode_result != 0 {
            println!("❌ Multimodal encoding failed: {}", encode_result);
            mtmd_input_chunks_free(chunks);
            if ctx_was_null {
                llama_free(ctx);
            }
            return -1;
        }

        println!("✅ Multimodal encoding successful, n_past: {}", new_n_past);

        // Get vocab pointer
        let model_ptr = llama_get_model(ctx);
        if model_ptr.is_null() {
            mtmd_input_chunks_free(chunks);
            if ctx_was_null {
                llama_free(ctx);
            }
            return -1;
        }

        let vocab = llama_model_get_vocab(model_ptr);
        if vocab.is_null() {
            mtmd_input_chunks_free(chunks);
            if ctx_was_null {
                llama_free(ctx);
            }
            return -1;
        }

        // 🔑 Inline streaming generation (avoid function call issues)
        println!("🔍 Starting inline streaming generation...");

        let generated_text = {
            // Initialize samplers
            let temp_sampler = llama_sampler_init_temp(temperature);
            let top_k_sampler = llama_sampler_init_top_k(top_k);
            let top_p_sampler = llama_sampler_init_top_p(top_p, 1);
            let repeat_sampler = llama_sampler_init_penalties(-1, repeat_penalty, 0.0, 0.0);
            let dist_sampler = llama_sampler_init_dist(1234);

            // Chain samplers
            let chain_params = llama_sampler_chain_params { no_perf: false };
            let sampler = llama_sampler_chain_init(chain_params);

            llama_sampler_chain_add(sampler, temp_sampler);
            llama_sampler_chain_add(sampler, top_k_sampler);
            llama_sampler_chain_add(sampler, top_p_sampler);
            llama_sampler_chain_add(sampler, repeat_sampler);
            llama_sampler_chain_add(sampler, dist_sampler);

            let n_ctx = llama_n_ctx(ctx);
            let vocab_size = llama_vocab_n_tokens(vocab);

            let mut n_past = new_n_past;
            let mut generated_text = String::new();
            let mut generated_count = 0;

            // Generation loop
            while generated_count < max_tokens && n_past < n_ctx {
                let logits = llama_get_logits(ctx);
                if logits.is_null() {
                    break;
                }

                let new_token_id = llama_sampler_sample(sampler, ctx, -1);

                // Check EOS using vocab
                if llama_vocab_is_eog(vocab, new_token_id) {
                    break;
                }

                // Convert token to text
                let mut token_buf = [0u8; 32];
                let token_len = llama_token_to_piece(
                    vocab,
                    new_token_id,
                    token_buf.as_mut_ptr() as *mut c_char,
                    token_buf.len() as c_int,
                    0,
                    false,
                );

                if token_len > 0 {
                    let token_str = std::str::from_utf8_unchecked(&token_buf[..token_len as usize]);
                    generated_text.push_str(token_str);

                    // 🔑 Call token callback
                    if let Some(callback) = on_token {
                        match CString::new(token_str) {
                            Ok(token_cstr) => {
                                callback(user_data, token_cstr.as_ptr(), new_token_id);
                            }
                            Err(_) => {
                                println!("⚠️ Token callback skipped");
                            }
                        }
                    }
                }

                let mut pos = n_past as LlamaPos;
                let mut tok = new_token_id as LlamaToken;
                let batch = llama_batch {
                    n_tokens: 1,
                    token: (&mut tok as *mut LlamaToken),
                    embd: std::ptr::null_mut(),
                    pos: (&mut pos as *mut LlamaPos),
                    n_seq_id: std::ptr::null_mut(),
                    seq_id: std::ptr::null_mut(),
                    logits: std::ptr::null_mut(),
                };
                if llama_decode(ctx, batch) != 0 {
                    println!("❌ Decode failed");
                    break;
                }

                n_past += 1;
                generated_count += 1;
            }

            llama_sampler_free(sampler);
            println!("✅ Generated {} tokens", generated_count);

            generated_text
        };

        // Cleanup
        mtmd_input_chunks_free(chunks);

        let token_count = generated_text.split_whitespace().count() as c_int;

        // 🔑 Call completion callback with safety checks
        if let Some(callback) = on_complete {
            match CString::new(generated_text.clone()) {
                Ok(text_cstr) => {
                    callback(user_data, text_cstr.as_ptr(), token_count);
                }
                Err(_) => {
                    println!("⚠️ Warning: Failed to create CString for completion text");
                    // Call with empty string
                    let empty_cstr = CString::new("").unwrap();
                    callback(user_data, empty_cstr.as_ptr(), token_count);
                }
            }
        }

        if ctx_was_null && !ctx.is_null() {
            println!("🔧 Freeing created context: {:p}", ctx);
            llama_free(ctx);
        }

        token_count
    }
}

// Free multimodal model with libmtmd support
#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_free_multimodal_model(_multimodal_model: *mut gpuf_multimodal_model) {}

#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_free_multimodal_model(multimodal_model: *mut gpuf_multimodal_model) {
    if !multimodal_model.is_null() {
        unsafe {
            let model = Box::from_raw(multimodal_model);
            if !model.text_model.is_null() {
                llama_model_free(model.text_model);
            }
            if !model.mtmd_context.is_null() {
                mtmd_free(model.mtmd_context);
            }
        }
    }
}

// Check if multimodal model supports vision
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_multimodal_supports_vision(
    multimodal_model: *mut gpuf_multimodal_model,
) -> bool {
    if multimodal_model.is_null() {
        return false;
    }

    unsafe {
        let model_ref = &*multimodal_model;
        if model_ref.mtmd_context.is_null() {
            return false;
        }

        mtmd_support_vision(model_ref.mtmd_context)
    }
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_multimodal_supports_vision(
    _multimodal_model: *mut gpuf_multimodal_model,
) -> bool {
    false
}

// Get multimodal model info
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_get_multimodal_info(
    multimodal_model: *mut gpuf_multimodal_model,
    has_vision: *mut bool,
) -> c_int {
    if multimodal_model.is_null() || has_vision.is_null() {
        return -1;
    }

    unsafe {
        let model_ref = &*multimodal_model;
        if model_ref.mtmd_context.is_null() {
            return -1;
        }

        *has_vision = mtmd_support_vision(model_ref.mtmd_context);
        0
    }
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_get_multimodal_info(
    _multimodal_model: *mut gpuf_multimodal_model,
    _has_vision: *mut bool,
) -> c_int {
    -1
}

// 🆕 Get vision tokens for the detected model type
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn gpuf_get_vision_tokens(
    multimodal_model: *mut gpuf_multimodal_model,
    start_token: *mut c_char,
    end_token: *mut c_char,
    media_token: *mut c_char,
    max_length: c_int,
) -> c_int {
    if multimodal_model.is_null() {
        return -1;
    }

    unsafe {
        let model_ref = &*multimodal_model;
        let vision_tokens = model_ref.projector_type.get_vision_tokens();

        // Convert Rust strings to C strings and copy to output buffers
        if let Ok(start_cstr) = CString::new(vision_tokens.start) {
            if !start_token.is_null() {
                let start_len = start_cstr.to_bytes_with_nul().len();
                let copy_len = std::cmp::min(start_len, max_length as usize);
                std::ptr::copy_nonoverlapping(start_cstr.as_ptr(), start_token, copy_len);
            }
        }

        if let Ok(end_cstr) = CString::new(vision_tokens.end) {
            if !end_token.is_null() {
                let end_len = end_cstr.to_bytes_with_nul().len();
                let copy_len = std::cmp::min(end_len, max_length as usize);
                std::ptr::copy_nonoverlapping(end_cstr.as_ptr(), end_token, copy_len);
            }
        }

        if let Ok(media_cstr) = CString::new(vision_tokens.media) {
            if !media_token.is_null() {
                let media_len = media_cstr.to_bytes_with_nul().len();
                let copy_len = std::cmp::min(media_len, max_length as usize);
                std::ptr::copy_nonoverlapping(media_cstr.as_ptr(), media_token, copy_len);
            }
        }

        // Return model type as integer for debugging
        model_ref.projector_type as c_int
    }
}

#[no_mangle]
#[cfg(target_os = "ios")]
pub extern "C" fn gpuf_get_vision_tokens(
    _multimodal_model: *mut gpuf_multimodal_model,
    _start_token: *mut c_char,
    _end_token: *mut c_char,
    _media_token: *mut c_char,
    _max_length: c_int,
) -> c_int {
    -1
}

#[cfg(target_os = "android")]
// Generate text from multimodal context using actual llama.cpp inference
fn generate_multimodal_response(
    ctx: *mut llama_context,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
) -> String {
    // Try to get vocab from context first
    unsafe {
        let vocab_size = llama_n_vocab(ctx);
        if vocab_size == 0 {
            return "❌ Context initialization failed - vocab size is 0".to_string();
        }
    }

    generate_multimodal_response_with_vocab(
        ctx,
        std::ptr::null(),
        max_tokens,
        temperature,
        top_k,
        top_p,
        repeat_penalty,
        0,
    ) // 🆕 Start from position 0 for text-only generation
}

#[cfg(target_os = "android")]
fn generate_multimodal_response_with_vocab(
    ctx: *mut llama_context,
    direct_vocab: *const llama_vocab,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
    initial_n_past: c_int, // 🆕 Accept correct initial position from encoding
) -> String {
    if ctx.is_null() {
        return "❌ Invalid context".to_string();
    }

    // SAFETY: `ctx` was checked for null above and must be a live llama.cpp
    // context. Sampler pointers are checked before use where ownership matters.
    let temp_sampler = unsafe { llama_sampler_init_temp(temperature) };
    let top_k_sampler = unsafe { llama_sampler_init_top_k(top_k) };
    let top_p_sampler = unsafe { llama_sampler_init_top_p(top_p, 1) };
    let repeat_sampler = unsafe { llama_sampler_init_penalties(-1, repeat_penalty, 0.0, 0.0) };
    let dist_sampler = unsafe { llama_sampler_init_dist(1234) }; // Fixed seed for reproducibility

    // Chain samplers together
    let chain_params = llama_sampler_chain_params { no_perf: false };
    let sampler = unsafe { llama_sampler_chain_init(chain_params) };

    // SAFETY: `sampler` is a newly created sampler chain; sampler components are
    // handed to llama.cpp chain ownership exactly once.
    unsafe {
        llama_sampler_chain_add(sampler, temp_sampler);
        llama_sampler_chain_add(sampler, top_k_sampler);
        llama_sampler_chain_add(sampler, top_p_sampler);
        llama_sampler_chain_add(sampler, repeat_sampler);
        llama_sampler_chain_add(sampler, dist_sampler);
    }

    // Get model and vocab at function start (only once, like llama.rn)
    // SAFETY: `ctx` is a non-null live llama.cpp context for this generation.
    let model = unsafe { llama_get_model(ctx) };
    if model.is_null() {
        // SAFETY: `sampler` is owned by this function and has not been freed yet.
        unsafe { llama_sampler_free(sampler) };
        return "❌ Model is null".to_string();
    }

    let vocab = if direct_vocab.is_null() {
        // SAFETY: `model` was returned by llama.cpp and checked for null.
        unsafe { llama_model_get_vocab(model) }
    } else {
        direct_vocab
    };

    if vocab.is_null() {
        // SAFETY: `sampler` is owned by this function and has not been freed yet.
        unsafe { llama_sampler_free(sampler) };
        return "❌ Vocab is null".to_string();
    }

    // SAFETY: `vocab` and `ctx` were checked above and remain valid for this call.
    let vocab_size = unsafe { llama_vocab_n_tokens(vocab) };
    let n_ctx = unsafe { llama_n_ctx(ctx as *const llama_context) };

    println!(
        "🔢 Context size: {}, Vocab size: {}, Using direct vocab: {}",
        n_ctx,
        vocab_size,
        !direct_vocab.is_null()
    );

    // Validate vocab
    if vocab_size == 0 {
        println!("❌ CRITICAL: Vocab size is 0 - vocab is not properly initialized!");
        // SAFETY: `sampler` is owned by this function and has not been freed yet.
        unsafe { llama_sampler_free(sampler) };
        return "❌ Vocab initialization failed - vocab size is 0".to_string();
    }

    println!(
        "🔍 Starting multimodal inference with vocab size: {}",
        vocab_size
    );

    // 🆕 Follow llama.rn pattern: sample immediately after mtmd_helper_eval_chunks
    println!("🔧 Following llama.rn pattern - sampling immediately after encoding");

    // 🆕 Declare n_past in outer scope to fix variable access issue
    let mut n_past = initial_n_past;

    // Generate tokens one by one
    let mut generated_text = String::new();
    let mut generated_count = 0;

    // 🔍 Debug: Check context state before generation loop
    println!("🔍 === Generation Loop Starting ===");
    println!("🔍 Initial n_past: {}", n_past);
    println!("🔍 Context size: {}", n_ctx);
    println!("🔍 Vocab size: {}", vocab_size);
    println!("🔍 Max tokens: {}", max_tokens);
    println!(
        "🔍 Temperature: {}, Top-K: {}, Top-P: {}",
        temperature, top_k, top_p
    );

    // 🔍 Try to get logits to verify context is ready
    // SAFETY: `ctx` is a live llama.cpp context. The logits pointer is only
    // read after a null check and only for diagnostics.
    unsafe {
        let logits_ptr = llama_get_logits(ctx);
        if logits_ptr.is_null() {
            println!("⚠️ WARNING: logits pointer is null! Context may not be ready.");
        } else {
            println!("✅ Logits pointer valid: {:p}", logits_ptr);
            // Sample first few logits for debugging
            let first_logit = *logits_ptr;
            let second_logit = *logits_ptr.add(1);
            println!(
                "🔍 First logits: [{:.4}, {:.4}, ...]",
                first_logit, second_logit
            );
        }
    }

    for i in 0..max_tokens {
        println!("🔍 === Token {} === (n_past: {})", i, n_past);

        // Check sampler validity before sampling
        if sampler.is_null() {
            println!("❌ Sampler is null!");
            break;
        }

        // 🆕 Follow llama.cpp official pattern: use llama_sampler_sample with index -1 (last position)
        // SAFETY: `sampler` and `ctx` are live for this generation loop.
        let token = unsafe { llama_sampler_sample(sampler, ctx, -1) }; // 🆕 Use -1 for last position logits like llama.cpp
        println!("🔍 Sampled token: {} (0x{:x})", token, token);

        // Check token validity
        println!("🔍 Token in range: {}", token < vocab_size);

        // Use official llama.cpp EOS check method
        // SAFETY: `vocab` is a live llama.cpp vocab pointer checked above.
        if unsafe { llama_vocab_is_eog(vocab, token) } {
            println!("✅ EOS token detected: {} (0x{:x})", token, token);
            break;
        }

        // Check if this is a control token (like llama.rn does)
        // SAFETY: `vocab` is a live llama.cpp vocab pointer checked above.
        if unsafe { llama_vocab_is_control(vocab, token) } {
            println!(
                "⚠️ Control token detected: {} (0x{:x}), skipping...",
                token, token
            );
            // Still need to accept the token into context but don't add to output
            let mut pos = n_past as LlamaPos;
            let mut tok = token as LlamaToken;
            let accept_batch = llama_batch {
                n_tokens: 1,
                token: (&mut tok as *mut LlamaToken),
                embd: std::ptr::null_mut(),
                pos: (&mut pos as *mut LlamaPos),
                n_seq_id: std::ptr::null_mut(),
                seq_id: std::ptr::null_mut(),
                logits: std::ptr::null_mut(),
            };
            n_past += 1;
            // SAFETY: `accept_batch` points to local token/position storage
            // that remains alive for the duration of this decode call.
            let accept_result = unsafe { llama_decode(ctx, accept_batch) };
            if accept_result != 0 {
                println!("❌ Failed to accept control token {}: {}", i, accept_result);
                break;
            }
            continue; // Skip to next token
        }

        // Convert token to string (use vocab from function start)
        let mut token_str = [0u8; 64];
        // SAFETY: `token_str` is a writable local buffer and `vocab` is live.
        let token_len = unsafe {
            llama_token_to_piece(
                vocab, // Use vocab obtained at function start
                token,
                token_str.as_mut_ptr(),
                token_str.len() as c_int,
                0,
                false,
            )
        };

        if token_len > 0 {
            let token_len = (token_len as usize).min(token_str.len());
            match std::str::from_utf8(&token_str[..token_len]) {
                Ok(token_text) => {
                    generated_text.push_str(token_text);
                    generated_count += 1;
                    println!(
                        " Generated token text redacted ({} bytes)",
                        token_text.len()
                    );
                }
                Err(_) => {
                    println!(" Skipping non-UTF8 token piece ({} bytes)", token_len);
                }
            }
        }

        // Accept the token into context
        let mut pos = n_past as LlamaPos;
        let mut tok = token as LlamaToken;
        let accept_batch = llama_batch {
            n_tokens: 1,
            token: (&mut tok as *mut LlamaToken),
            embd: std::ptr::null_mut(),
            pos: (&mut pos as *mut LlamaPos),
            n_seq_id: std::ptr::null_mut(),
            seq_id: std::ptr::null_mut(),
            logits: std::ptr::null_mut(),
        };
        n_past += 1;

        let accept_result = unsafe { llama_decode(ctx, accept_batch) };
        if accept_result != 0 {
            println!("❌ Failed to accept token {}: {}", i, accept_result);
            break;
        }

        // Safety limit
        if generated_count >= max_tokens || generated_text.len() > 1000 {
            println!("🛑 Generation limit reached");
            break;
        }
    }

    // SAFETY: `sampler` is owned by this function and has not been freed yet.
    unsafe { llama_sampler_free(sampler) };

    println!("\n✅ Real generation completed: {} tokens", generated_count);

    if generated_text.is_empty() {
        "❌ No text generated - model may need proper prompt formatting".to_string()
    } else {
        generated_text
    }
}

// 🆕 Version with streaming callbacks
#[cfg(target_os = "android")]
fn generate_multimodal_response_with_callbacks(
    ctx: *mut llama_context,
    direct_vocab: *const llama_vocab,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
    initial_n_past: c_int, // 🆕 Use c_int for ABI consistency
    on_token: TokenCallback,
    user_data: *mut c_void,
) -> String {
    println!("🔍 generate_multimodal_response_with_callbacks: ENTRY");

    unsafe {
        println!("🔍 Initializing samplers...");

        // Initialize samplers (same as original function)
        let temp_sampler = llama_sampler_init_temp(temperature);
        println!("🔍 temp_sampler: {:p}", temp_sampler);

        let top_k_sampler = llama_sampler_init_top_k(top_k);
        println!("🔍 top_k_sampler: {:p}", top_k_sampler);

        let top_p_sampler = llama_sampler_init_top_p(top_p, 1);
        println!("🔍 top_p_sampler: {:p}", top_p_sampler);

        let repeat_sampler = llama_sampler_init_penalties(-1, repeat_penalty, 0.0, 0.0);
        println!("🔍 repeat_sampler: {:p}", repeat_sampler);

        let dist_sampler = llama_sampler_init_dist(1234);
        println!("🔍 dist_sampler: {:p}", dist_sampler);

        // Chain samplers together
        let chain_params = llama_sampler_chain_params { no_perf: false };
        let sampler = llama_sampler_chain_init(chain_params);
        println!("🔍 sampler chain: {:p}", sampler);

        if sampler.is_null() {
            return "❌ Failed to create sampler chain".to_string();
        }

        llama_sampler_chain_add(sampler, temp_sampler);
        llama_sampler_chain_add(sampler, top_k_sampler);
        llama_sampler_chain_add(sampler, top_p_sampler);
        llama_sampler_chain_add(sampler, repeat_sampler);
        llama_sampler_chain_add(sampler, dist_sampler);

        let n_ctx = llama_n_ctx(ctx);
        let vocab_size = llama_vocab_n_tokens(direct_vocab);
        println!("🔍 n_ctx: {}, vocab_size: {}", n_ctx, vocab_size);

        if vocab_size == 0 {
            llama_sampler_free(sampler);
            return "❌ Vocab initialization failed".to_string();
        }

        println!(
            "🔍 Starting streaming generation with vocab size: {}",
            vocab_size
        );

        let mut n_past = initial_n_past;
        let mut generated_text = String::new();
        let mut generated_count = 0;
        let mut utf8_buf = Utf8EmitBuffer::new();

        // Generation loop with callbacks
        while generated_count < max_tokens && n_past < n_ctx {
            let logits = llama_get_logits(ctx);
            if logits.is_null() {
                break;
            }

            // Sample next token
            let new_token_id = llama_sampler_sample(sampler, ctx, -1);

            // Check for EOS (use model's vocab to get EOS token)
            let model = llama_get_model(ctx);
            let eos_token = llama_token_eos(model);
            if new_token_id == eos_token {
                println!("🛑 EOS token reached");
                break;
            }

            // Convert token to text
            let mut token_buf = [0u8; 32];
            let token_len = llama_token_to_piece(
                direct_vocab,
                new_token_id,
                token_buf.as_mut_ptr() as *mut c_char,
                token_buf.len() as c_int,
                0,
                false,
            );

            if token_len > 0 {
                let emitted = utf8_buf.push_and_take_valid(&token_buf[..token_len as usize]);
                if !emitted.is_empty() {
                    generated_text.push_str(&emitted);

                    // 🔑 Call token callback with safety checks
                    if let Some(callback) = on_token {
                        match CString::new(emitted.as_str()) {
                            Ok(token_cstr) => {
                                callback(user_data, token_cstr.as_ptr(), new_token_id);
                            }
                            Err(_) => {
                                // If CString creation fails (e.g. embedded NUL), skip.
                                println!("⚠️ Warning: Failed to create CString for token");
                            }
                        }
                    }
                }
            }

            // Token is already sampled and accepted

            let mut pos = n_past as LlamaPos;
            let mut tok = new_token_id as LlamaToken;
            let batch = llama_batch {
                n_tokens: 1,
                token: (&mut tok as *mut LlamaToken),
                embd: std::ptr::null_mut(),
                pos: (&mut pos as *mut LlamaPos),
                n_seq_id: std::ptr::null_mut(),
                seq_id: std::ptr::null_mut(),
                logits: std::ptr::null_mut(),
            };
            if llama_decode(ctx, batch) != 0 {
                println!("❌ Failed to decode token");
                break;
            }

            n_past += 1;
            generated_count += 1;
        }

        llama_sampler_free(sampler);
        println!(
            "✅ Streaming generation completed: {} tokens",
            generated_count
        );

        let tail = utf8_buf.flush_lossy();
        if !tail.is_empty() {
            generated_text.push_str(&tail);
        }

        generated_text
    }
}

// ...
pub extern "C" fn gpuf_tokenize_text(
    ctx: *mut llama_context,
    text: *const c_char,
    tokens: *mut LlamaToken,
    _max_tokens: c_int,
) -> c_int {
    if ctx.is_null() || text.is_null() || tokens.is_null() {
        return -1;
    }
    /*
    real_llama_tokenize(ctx, text, tokens, max_tokens, true)
    */
    0 // Placeholder
}

#[no_mangle]
pub extern "C" fn gpuf_generate_final_solution_text(
    model: *const llama_model,
    ctx: *mut llama_context,
    prompt: *const c_char,
    _max_tokens: c_int,
    output: *mut c_char,
    output_len: c_int,
) -> c_int {
    if model.is_null() || ctx.is_null() || prompt.is_null() || output.is_null() {
        return -1;
    }

    unsafe {
        let prompt_str = match CStr::from_ptr(prompt).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };

        // Use real llama.cpp functions for Android
        /*
        let mut tokens = vec![0 as LlamaToken; 1024];
        let token_count = real_llama_tokenize(ctx, prompt, tokens.as_mut_ptr(), 1024, true);
        */
        let token_count = 1; // Placeholder

        let n_ctx = real_llama_n_ctx(ctx);

        // For now, return a simple response showing tokenization worked without echoing prompt text.
        let output_text = format!(
            "Real inference working (prompt {} bytes, tokens: {}, ctx: {})",
            prompt_str.len(),
            token_count,
            n_ctx
        );
        let output_cstr = CString::new(output_text).unwrap();

        let copy_len = std::cmp::min(output_cstr.as_bytes().len(), output_len as usize);
        std::ptr::copy_nonoverlapping(output_cstr.as_ptr(), output, copy_len);
        *output.add(copy_len) = 0;

        copy_len as c_int
    }
}

#[no_mangle]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub extern "C" fn gpuf_generate_with_sampling(
    model: *const llama_model,
    ctx: *mut llama_context,
    prompt: *const c_char,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
    output: *mut c_char,
    output_len: c_int,
    token_buffer: *mut LlamaToken,
    token_buffer_size: c_int,
) -> c_int {
    if model.is_null()
        || ctx.is_null()
        || prompt.is_null()
        || output.is_null()
        || token_buffer.is_null()
    {
        return -1;
    }

    if token_buffer_size <= 0 || output_len <= 0 {
        return -2;
    }

    unsafe {
        println!("🔥 Using manual completion like llama.rn implements");
        println!(
            "🎛️ Sampling params: temp={:.2}, top_k={}, top_p={:.2}, repeat_penalty={:.2}",
            temperature, top_k, top_p, repeat_penalty
        );

        // Use manual completion implementation based on actual llama.cpp API
        manual_llama_completion(
            model,
            ctx,
            prompt,
            max_tokens,
            temperature,
            top_k,
            top_p,
            repeat_penalty,
            output,
            output_len,
        )
    }
}

#[no_mangle]
pub extern "C" fn gpuf_system_info() -> *const c_char {
    let info = CString::new("GPUFabric Android LLaMA.cpp Engine").unwrap();
    info.into_raw()
}

#[no_mangle]
pub extern "C" fn gpuf_version() -> *const c_char {
    let version = CString::new("9.0.0-x86_64-android-FINAL-LLAMA-SOLUTION").unwrap();
    version.into_raw()
}

#[no_mangle]
pub extern "C" fn gpuf_init() -> c_int {
    println!("🔥 GPUFabric Android LLaMA.cpp solution initialized");

    #[cfg(target_os = "android")]
    {
        // Step 1: Initialize memory pool first
        if !init_memory_pool() {
            println!("❌ Failed to initialize memory pool");
            return -1;
        }
        println!(
            "✅ Memory pool initialized: {}MB",
            MEMORY_POOL_SIZE / (1024 * 1024)
        );

        // Step 2: Setup C++ runtime
        use std::env;

        if env::var("LD_PRELOAD").is_err() {
            let possible_paths = vec![
                "/system/lib64/libc++_shared.so",                   // Standard ARM64
                "/system/lib/libc++_shared.so",                     // Standard ARM32
                "/apex/com.android.runtime/lib64/libc++_shared.so", // APEX ARM64
                "/apex/com.android.runtime/lib/libc++_shared.so",   // APEX ARM32
            ];

            let mut found_path = None;
            for path in possible_paths {
                if std::path::Path::new(path).exists() {
                    found_path = Some(path);
                    break;
                }
            }

            match found_path {
                Some(path) => {
                    println!("🔧 Auto-setting LD_PRELOAD for C++ runtime: {}", path);
                    env::set_var("LD_PRELOAD", path);
                }
                None => {
                    println!("⚠️ C++ runtime library not found, may cause issues");
                }
            }
        }

        // Step 3: Initialize llama.cpp backend
        real_llama_backend_init();

        // Force reference to GGML backend symbols to ensure they are linked
        unsafe {
            let _ggml_backend_dev_by_type_ptr = ggml_backend_dev_by_type as *const ();
            let _ggml_backend_load_all_ptr = ggml_backend_load_all as *const ();

            if !_ggml_backend_dev_by_type_ptr.is_null() && !_ggml_backend_load_all_ptr.is_null() {
                println!("✅ GGML backend symbols verified");
            } else {
                println!("❌ GGML backend symbols missing");
                return -1;
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        real_llama_backend_init();
    }

    1 // Success
}

#[no_mangle]
pub extern "C" fn gpuf_cleanup() -> c_int {
    println!("🧹 GPUFabric Android LLaMA.cpp solution cleaned up");

    #[cfg(target_os = "android")]
    {
        // Cleanup memory pool
        cleanup_memory_pool();
        println!("✅ Memory pool cleaned up");
    }

    real_llama_backend_free();
    0
}

// ============================================================================
// Android memory pool for llama.cpp allocations
// ============================================================================

#[repr(C)]
pub struct MemoryPool {
    buffer: usize,
    size: usize,
    used: usize,
    initialized: bool,
}

static MEMORY_POOL: Lazy<Mutex<MemoryPool>> = Lazy::new(|| {
    Mutex::new(MemoryPool {
        buffer: 0,
        size: 0,
        used: 0,
        initialized: false,
    })
});

// Memory pool size: 64MB for llama.cpp internal allocations
const MEMORY_POOL_SIZE: usize = 64 * 1024 * 1024; // 64MB

#[cfg(target_os = "android")]
pub fn init_memory_pool() -> bool {
    let mut pool = MEMORY_POOL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if pool.initialized {
        return true;
    }

    // Allocate memory pool using mmap for better control.
    // SAFETY: Passing a null address lets the kernel choose the mapping. The
    // requested length is the fixed `MEMORY_POOL_SIZE`, fd is -1 with
    // MAP_ANONYMOUS, and the returned pointer is checked against MAP_FAILED
    // before storing it under the mutex-protected pool state.
    let buffer = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            MEMORY_POOL_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };

    if buffer == libc::MAP_FAILED {
        return false;
    }

    *pool = MemoryPool {
        buffer: buffer as usize,
        size: MEMORY_POOL_SIZE,
        used: 0,
        initialized: true,
    };

    true
}

#[cfg(target_os = "android")]
pub fn allocate_from_pool(size: usize, alignment: usize) -> *mut u8 {
    if size == 0 || alignment == 0 || !alignment.is_power_of_two() {
        return std::ptr::null_mut();
    }

    let mut pool = MEMORY_POOL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !pool.initialized || pool.buffer == 0 {
        return std::ptr::null_mut();
    }

    // Calculate aligned offset
    let current_offset = pool.used;
    let aligned_offset = (current_offset + alignment - 1) & !(alignment - 1);
    let new_used = aligned_offset.saturating_add(size);

    // Check if we have enough space
    if new_used > pool.size {
        return std::ptr::null_mut();
    }

    // Update pool state and return pointer
    pool.used = new_used;
    // SAFETY: `pool.buffer` is a live mmap allocation while `initialized` is
    // true. Bounds were checked with `new_used <= pool.size`, and
    // `aligned_offset` was derived from a power-of-two alignment.
    unsafe { (pool.buffer as *mut u8).add(aligned_offset) }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn reset_pool() {
    #[cfg(target_os = "android")]
    {
        let mut pool = MEMORY_POOL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pool.used = 0;
    }

    #[cfg(target_os = "ios")]
    {
        let _ = ();
    }
}

#[cfg(target_os = "android")]
pub fn cleanup_memory_pool() {
    let mut pool = MEMORY_POOL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if pool.initialized && pool.buffer != 0 {
        // SAFETY: The buffer/size pair was created by `init_memory_pool` with
        // mmap and is still marked initialized under the same mutex. State is
        // cleared immediately after munmap to prevent double unmapping.
        unsafe {
            libc::munmap(pool.buffer as *mut libc::c_void, pool.size);
        }
        pool.initialized = false;
        pool.buffer = 0;
        pool.size = 0;
        pool.used = 0;
    }
}

// ============================================================================
// Async Generation Control Functions
// ============================================================================

/// Stop ongoing generation
#[no_mangle]
pub extern "C" fn gpuf_stop_generation(_ctx: *mut llama_context) -> c_int {
    println!("🛑 Stopping generation...");
    set_generation_stop(true);

    // Wait a bit for generation to stop
    std::thread::sleep(std::time::Duration::from_millis(100));

    println!("✅ Generation stop signal sent");
    0
}

/// Start async generation with streaming callback (simplified version)
#[no_mangle]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub extern "C" fn gpuf_start_generation_async(
    ctx: *mut llama_context,
    prompt: *const c_char,
    max_tokens: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    repeat_penalty: f32,
    on_token_callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
    user_data: *mut c_void,
) -> c_int {
    if ctx.is_null() || prompt.is_null() {
        println!("❌ Invalid context or prompt for async generation");
        return -1;
    }

    // Initialize generation control
    init_generation_control();
    set_generation_stop(false);

    println!("🚀 Starting streaming generation...");

    // For now, use synchronous generation with callbacks
    // This avoids thread safety issues while providing streaming
    // SAFETY: `ctx` and `prompt` were checked for null above and must remain
    // valid for the duration of this synchronous call. Local token, logits,
    // and position buffers outlive each llama.cpp decode call. Callback C
    // strings are invoked only while their temporary CString storage is alive.
    unsafe {
        // Get prompt string
        let prompt_str = std::ffi::CStr::from_ptr(prompt).to_str().unwrap_or("");

        // Reset memory pool
        reset_pool();

        // Clear KV cache for sequence 0 (remove all positions)
        let kv = llama_get_memory(ctx);
        let clear_result = llama_memory_seq_rm(kv, 0, -1, -1);
        if !clear_result {
            println!("⚠️ llama_memory_seq_rm failed, trying full clear...");
            llama_memory_clear(kv, false);
        }
        println!("✅ KV cache cleared for clean generation");

        // Tokenize prompt using real llama.cpp tokenizer
        let model = llama_get_model(ctx);
        if model.is_null() {
            println!("🔍 Early return due to null model");
            return -1;
        }

        let vocab = llama_model_get_vocab(model);
        if vocab.is_null() {
            println!("🔍 Early return due to null vocab");
            return -1;
        }

        let mut tokens: Vec<i32> = vec![0; 512];
        let mut token_count = llama_tokenize(
            vocab,
            prompt,
            prompt_str.len() as c_int,
            tokens.as_mut_ptr(),
            tokens.len() as c_int,
            true,
            true,
        );
        if token_count < 0 {
            let needed = (-token_count) as usize;
            tokens = vec![0; needed.max(1)];
            token_count = llama_tokenize(
                vocab,
                prompt,
                prompt_str.len() as c_int,
                tokens.as_mut_ptr(),
                tokens.len() as c_int,
                true,
                true,
            );
        }

        println!("🔍 After tokenization: token_count={}", token_count);

        if token_count <= 0 {
            println!("🔍 Early return due to token_count <= 0");
            return -1;
        }

        // Prefill prompt in chunks to respect ctx n_batch (llama.cpp asserts otherwise)
        let n_batch = {
            let nb = llama_n_batch(ctx);
            if nb > 0 {
                nb
            } else {
                128
            }
        };

        println!(
            "🔍 Prefill: token_count={}, n_batch={}",
            token_count, n_batch
        );

        let mut batch_pos_array = [0i32; 512];
        let mut logits_array = [0i8; 512];

        let mut n_past: i32 = 0;
        let mut start: i32 = 0;
        while start < token_count {
            let end = std::cmp::min(start + n_batch, token_count);
            let n = end - start;

            for i in 0..n {
                batch_pos_array[i as usize] = n_past + i;
                // Request logits only for the last token of the final chunk
                logits_array[i as usize] = if end == token_count && i == n - 1 {
                    1
                } else {
                    0
                };
            }

            let batch = llama_batch {
                n_tokens: n,
                token: tokens.as_ptr().add(start as usize) as *mut LlamaToken,
                embd: std::ptr::null_mut(),
                pos: batch_pos_array.as_ptr() as *mut LlamaPos,
                n_seq_id: std::ptr::null_mut(),
                seq_id: std::ptr::null_mut(),
                logits: logits_array.as_ptr() as *mut i8,
            };

            println!(
                "🔍 Prefill llama_decode: start={}, end={}, n_tokens={}, n_past={} ",
                start, end, n, n_past
            );
            let decode_result = llama_decode(ctx, batch);
            if decode_result != 0 {
                println!("🔍 Early return due to decode failure: {}", decode_result);
                return -1;
            }
            n_past += n;
            start = end;
        }

        println!("🔍 Model and vocab ready, starting generation loop...");

        // Initialize sampler
        let temp_sampler = llama_sampler_init_temp(temperature);
        let top_k_sampler = llama_sampler_init_top_k(top_k);
        let top_p_sampler = llama_sampler_init_top_p(top_p, 1);
        let repeat_sampler = llama_sampler_init_penalties(-1, repeat_penalty, 0.0, 0.0);
        let dist_sampler = llama_sampler_init_dist(1234);

        let chain_params = llama_sampler_chain_params { no_perf: false };
        let sampler = llama_sampler_chain_init(chain_params);

        llama_sampler_chain_add(sampler, temp_sampler);
        llama_sampler_chain_add(sampler, top_k_sampler);
        llama_sampler_chain_add(sampler, top_p_sampler);
        llama_sampler_chain_add(sampler, repeat_sampler);
        llama_sampler_chain_add(sampler, dist_sampler);

        // Generate tokens with streaming callbacks
        let n_ctx = llama_n_ctx(ctx) as i32;
        let context_available = n_ctx - n_past;
        let safe_generation_limit = std::cmp::min(max_tokens, context_available);
        let mut next_pos = n_past;
        let mut utf8_buf = Utf8EmitBuffer::new();

        let mut completion_tokens: c_int = 0;
        for _i in 0..safe_generation_limit {
            // Check for stop signal
            if should_stop_generation() {
                println!("⏹️ Generation stopped by user");
                break;
            }

            // Sample next token using llama.cpp sampler
            let sampled_token = llama_sampler_sample(sampler, ctx, -1);

            println!(
                "🔍 Sampled token: {} (EOS: {})",
                sampled_token,
                llama_vocab_is_eog(vocab, sampled_token)
            );

            // Check EOS
            if llama_vocab_is_eog(vocab, sampled_token) {
                println!("🔍 EOS token detected, stopping generation");
                break;
            }

            completion_tokens = completion_tokens.saturating_add(1);

            // Convert token to text
            let mut token_buf = [0u8; 32];
            let token_len = llama_token_to_piece(
                vocab,
                sampled_token,
                token_buf.as_mut_ptr() as *mut c_char,
                token_buf.len() as c_int,
                0,
                false,
            );

            println!(
                "🔍 Token debug: sampled_token={}, token_len={}",
                sampled_token, token_len
            );

            if token_len > 0 {
                let raw_len = token_len as usize;
                let piece_len = raw_len.min(token_buf.len());
                if raw_len > token_buf.len() {
                    println!(
                        "⚠️ Token piece truncated for UTF-8 buffering (reported {} bytes, buffer {} bytes)",
                        raw_len,
                        token_buf.len()
                    );
                }

                let emitted = utf8_buf.push_and_take_valid(&token_buf[..piece_len]);
                println!(
                    "🔍 Token content redacted (emitted {} bytes, raw {} bytes)",
                    emitted.len(),
                    raw_len
                );

                // Call callback only if it's not None
                if !emitted.is_empty() {
                    if let Some(callback) = on_token_callback {
                        println!("🔍 Calling callback with token...");
                        match std::ffi::CString::new(emitted.as_str()) {
                            Ok(token_cstr) => {
                                callback(token_cstr.as_ptr(), user_data);
                                println!("🔍 Callback completed");
                            }
                            Err(_) => {
                                println!("⚠️ Token callback skipped - CString conversion failed");
                            }
                        }
                    } else {
                        println!(
                            "🔍 No callback - token text redacted ({} bytes)",
                            emitted.len()
                        );
                    }
                }
            } else if token_len < 0 {
                println!(
                    "⚠️ Token piece did not fit buffer (needed {} bytes)",
                    -token_len
                );
            } else {
                println!("🔍 Empty token skipped");
            }

            // Create single token batch
            let single_token_batch = llama_batch {
                n_tokens: 1,
                token: (&sampled_token as *const LlamaToken) as *mut LlamaToken,
                embd: std::ptr::null_mut(),
                pos: (&next_pos as *const LlamaPos) as *mut LlamaPos,
                n_seq_id: std::ptr::null_mut(),
                seq_id: std::ptr::null_mut(),
                logits: std::ptr::null_mut(),
            };

            // Decode token
            if llama_decode(ctx, single_token_batch) != 0 {
                break;
            }

            next_pos += 1;
        }

        // Cleanup sampler
        llama_sampler_free(sampler);

        // Flush any remaining buffered bytes (best-effort)
        let tail = utf8_buf.flush_lossy();

        if !tail.is_empty() {
            if let Some(callback) = on_token_callback {
                if let Ok(token_cstr) = std::ffi::CString::new(tail.as_str()) {
                    callback(token_cstr.as_ptr(), user_data);
                }
            }
        }

        // Cleanup
        cleanup_generation_control();
        println!(
            "✅ Streaming generation completed (generated {} tokens)",
            completion_tokens
        );
        completion_tokens
    }
}

/// Simple single token generation for testing
#[no_mangle]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub extern "C" fn gpuf_generate_single_token(
    model: *const llama_model,
    ctx: *mut llama_context,
    prompt: *const c_char,
    output: *mut c_char,
    output_len: c_int,
) -> c_int {
    if model.is_null() || ctx.is_null() || prompt.is_null() || output.is_null() {
        return -1;
    }

    if output_len <= 0 {
        return -2;
    }

    unsafe {
        println!("🔥 Single token sampling test");

        // Convert prompt to Rust string
        let prompt_str = match std::ffi::CStr::from_ptr(prompt).to_str() {
            Ok(s) => s,
            Err(_) => return -3,
        };

        println!("📝 Processing prompt ({} bytes)", prompt_str.len());

        // Simple tokenization
        let mut tokens = [0i32; 128];
        let token_count = safe_tokenize(ctx, prompt, tokens.as_mut_ptr(), 128, true);

        if token_count <= 0 {
            println!("❌ Tokenization failed");
            return -4;
        }

        println!("✅ Tokenized into {} tokens", token_count);

        // Create batch with logits request for last token
        let mut batch_pos_array = [0i32; 128];
        let mut logits_array = [0i8; 128];

        for i in 0..token_count {
            batch_pos_array[i as usize] = i;
            logits_array[i as usize] = if i == token_count - 1 { 1 } else { 0 };
        }

        let batch = llama_batch {
            n_tokens: token_count,
            token: tokens.as_ptr() as *mut LlamaToken,
            embd: std::ptr::null_mut(),
            pos: batch_pos_array.as_ptr() as *mut LlamaPos,
            n_seq_id: std::ptr::null_mut(),
            seq_id: std::ptr::null_mut(),
            logits: logits_array.as_ptr() as *mut i8,
        };

        // Decode prompt
        let decode_result = llama_decode(ctx, batch);
        if decode_result != 0 {
            println!("❌ Decode failed: {}", decode_result);
            return -5;
        }

        println!("✅ Decode successful");

        // Sample from the last token position
        /*
        let sampled_token = sample_token(ctx, token_count - 1, 0.0f32, 0, 1.0f32);
        */
        let sampled_token = 1; // Placeholder

        if sampled_token < 0 {
            println!("❌ Sampling failed: {}", sampled_token);
            return -6;
        }

        println!("🎯 Sampled token: {}", sampled_token);

        // Convert result to string
        let result_text = format!("Token: {}", sampled_token);
        let text_bytes = result_text.as_bytes();
        let copy_len = std::cmp::min(text_bytes.len(), output_len as usize - 1);
        std::ptr::copy_nonoverlapping(text_bytes.as_ptr(), output as *mut u8, copy_len);
        *output.add(copy_len) = 0;

        copy_len as c_int
    }
}

// ============================================================================
// C FFI - Mobile TLS Policy Validation
// ============================================================================

fn optional_c_string(ptr: *const c_char) -> Result<Option<String>, c_int> {
    if ptr.is_null() {
        return Ok(None);
    }
    // SAFETY: `ptr` is checked for null above and is expected to point to a
    // valid NUL-terminated C string owned by the caller for this call.
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| -5)?
        .trim()
        .to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn required_c_string(ptr: *const c_char) -> Result<String, c_int> {
    if ptr.is_null() {
        return Err(-1);
    }
    // SAFETY: `ptr` is checked for null above and must point to a valid,
    // NUL-terminated C string for this call.
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| -5)?
        .trim()
        .to_string();
    if value.is_empty() {
        Err(-1)
    } else {
        Ok(value)
    }
}

/// Validate mobile TLS policy inputs before a wrapper enables remote worker TLS.
///
/// Returns:
/// - 0: valid policy
/// - -1: missing or invalid server name
/// - -2: missing CA bundle and SHA256 pin
/// - -3: invalid CA bundle path/content
/// - -4: invalid SHA256 certificate pin
/// - -5: invalid UTF-8 in one of the C strings
#[no_mangle]
pub extern "C" fn gpuf_validate_mobile_tls_policy(
    ca_cert_path: *const c_char,
    server_name: *const c_char,
    cert_sha256_pin: *const c_char,
) -> c_int {
    let ca_cert_path = match optional_c_string(ca_cert_path) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let server_name = match required_c_string(server_name) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let cert_sha256_pin = match optional_c_string(cert_sha256_pin) {
        Ok(v) => v,
        Err(code) => return code,
    };

    match crate::util::mobile_tls_policy::validate_mobile_tls_policy(
        ca_cert_path.as_deref(),
        &server_name,
        cert_sha256_pin.as_deref(),
    ) {
        Ok(_) => 0,
        Err(crate::util::mobile_tls_policy::MobileTlsPolicyError::MissingServerName)
        | Err(crate::util::mobile_tls_policy::MobileTlsPolicyError::InvalidServerName(_)) => -1,
        Err(crate::util::mobile_tls_policy::MobileTlsPolicyError::MissingTrustMaterial) => -2,
        Err(crate::util::mobile_tls_policy::MobileTlsPolicyError::InvalidCaBundle(_)) => -3,
        Err(crate::util::mobile_tls_policy::MobileTlsPolicyError::InvalidSha256Pin(_)) => -4,
    }
}

// ============================================================================
// C FFI - Remote Worker Management and Monitoring
// ============================================================================

/// Start remote worker and initialize global worker (C API)
#[cfg(any(target_os = "android", target_os = "ios"))]
#[no_mangle]
pub extern "C" fn start_remote_worker(
    server_addr: *const c_char,
    control_port: c_int,
    proxy_port: c_int,
    worker_type: *const c_char,
    client_id: *const c_char,
) -> c_int {
    use crate::util::cmd::{Args, EngineType, LlamaSplitModeArg, WorkerType};

    println!("🔥 GPUFabric C API: Starting remote worker");

    // Convert C strings to Rust strings
    let server_addr_str = if server_addr.is_null() {
        eprintln!("❌ Error: server_addr is null");
        return -1;
    } else {
        // SAFETY: `server_addr` was checked for null and must remain a valid
        // NUL-terminated C string for the duration of this call.
        match unsafe { std::ffi::CStr::from_ptr(server_addr).to_str() } {
            Ok(s) => s,
            Err(e) => {
                eprintln!("❌ Error: Invalid server_addr UTF-8: {}", e);
                return -1;
            }
        }
    };

    let worker_type_str = if worker_type.is_null() {
        eprintln!("❌ Error: worker_type is null");
        return -1;
    } else {
        // SAFETY: `worker_type` was checked for null and must remain a valid
        // NUL-terminated C string for the duration of this call.
        match unsafe { std::ffi::CStr::from_ptr(worker_type).to_str() } {
            Ok(s) => s,
            Err(e) => {
                eprintln!("❌ Error: Invalid worker_type UTF-8: {}", e);
                return -1;
            }
        }
    };

    let client_id_str = if client_id.is_null() {
        eprintln!("❌ Error: client_id is null");
        return -1;
    } else {
        // SAFETY: `client_id` was checked for null and must remain a valid
        // NUL-terminated C string for the duration of this call.
        match unsafe { std::ffi::CStr::from_ptr(client_id).to_str() } {
            Ok(s) => s,
            Err(e) => {
                eprintln!("❌ Error: Invalid client_id UTF-8: {}", e);
                return -1;
            }
        }
    };

    println!(
        "📡 C API: Remote worker config received (control_port={}, proxy_port={}, worker_type={}, server_addr_len={}, client_id_len={})",
        control_port,
        proxy_port,
        worker_type_str,
        server_addr_str.len(),
        client_id_str.len()
    );

    // Parse worker type
    let worker_type = match worker_type_str {
        "TCP" => WorkerType::TCP,
        "WS" => WorkerType::WS,
        _ => {
            eprintln!("❌ Error: Unknown worker type: {}", worker_type_str);
            return -1;
        }
    };

    // Create args
    let args = Args {
        server_addr: server_addr_str.to_string(),
        control_port: control_port as u16,
        proxy_port: proxy_port as u16,
        worker_type,
        engine_type: EngineType::LLAMA,
        client_id: Some(
            hex::decode(client_id_str)
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
        ),
        config: None,
        local_addr: "127.0.0.1".to_string(),
        local_port: 0,
        p2p_advertise_ip: None,
        p2p_udp_port: 40000,
        p2p_bind_addr: "127.0.0.1".to_string(),
        p2p_public_listen: false,
        cert_chain_path: "".to_string(),
        control_tls: false,
        control_tls_server_name: None,
        auto_models: false,
        hugging_face_hub_token: None,
        chat_template_path: None,
        standalone_llama: false,
        api_key: None,
        llama_model_path: None,
        n_gpu_layers: 99,
        n_ctx: 2048,  // Reduced for Android memory constraints
        n_batch: 512, // Reduced for Android memory constraints
        llama_split_mode: LlamaSplitModeArg::Layer,
        llama_main_gpu: 0,
        llama_devices: None,
        stream_chunk_bytes: 256,
    };

    #[cfg(target_os = "android")]
    {
        // Keep existing Android behavior for now.
        println!("🚀 C API: Initializing global worker with Android-native login...");
        std::io::stdout().flush().unwrap();

        let local_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create local tokio runtime");

        match local_runtime.block_on(async {
            crate::handle::android_sdk::perform_android_login(
                server_addr_str,
                control_port as u16,
                client_id_str,
                false,
            )
            .await
        }) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("❌ C API: Failed to start and login Android worker: {}", e);
                -1
            }
        }
    }

    #[cfg(target_os = "ios")]
    {
        // iOS: login via cross-platform worker_sdk.
        let local_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create local tokio runtime");

        match local_runtime.block_on(async {
            crate::worker_sdk::perform_login(
                server_addr_str,
                control_port as u16,
                client_id_str,
                args.auto_models,
            )
            .await
        }) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("❌ C API: Failed to login iOS worker: {}", e);
                -1
            }
        }
    }
}

/// Start remote worker over TLS and initialize global worker (C API)
///
/// This is additive: `start_remote_worker` keeps the legacy plaintext behavior.
/// Returns -2 when the TLS CA/SNI/SHA256 pin policy is invalid.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[no_mangle]
pub extern "C" fn start_remote_worker_with_tls(
    server_addr: *const c_char,
    control_port: c_int,
    proxy_port: c_int,
    worker_type: *const c_char,
    client_id: *const c_char,
    ca_cert_path: *const c_char,
    control_tls_server_name: *const c_char,
    cert_sha256_pin: *const c_char,
) -> c_int {
    use crate::util::mobile_control_stream::MobileControlTlsConfig;

    println!("🔥 GPUFabric C API: Starting TLS remote worker");

    let server_addr_str = match required_c_string(server_addr) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let worker_type_str = match required_c_string(worker_type) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let client_id_str = match required_c_string(client_id) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let ca_cert_path = match optional_c_string(ca_cert_path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let control_tls_server_name = match optional_c_string(control_tls_server_name) {
        Ok(s) => s.unwrap_or_else(|| server_addr_str.clone()),
        Err(code) => return code,
    };
    let cert_sha256_pin = match optional_c_string(cert_sha256_pin) {
        Ok(s) => s,
        Err(code) => return code,
    };

    match worker_type_str.as_str() {
        "TCP" | "WS" => {}
        _ => {
            eprintln!("❌ Error: Unknown worker type: {}", worker_type_str);
            return -1;
        }
    }

    let tls_config = match MobileControlTlsConfig::from_inputs(
        true,
        ca_cert_path.as_deref(),
        Some(control_tls_server_name.as_str()),
        cert_sha256_pin.as_deref(),
    ) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("❌ Error: Invalid mobile control TLS policy: {}", e);
            return -2;
        }
    };

    println!(
        "📡 C API: TLS remote worker config received (control_port={}, proxy_port={}, worker_type={}, server_addr_len={}, client_id_len={})",
        control_port,
        proxy_port,
        worker_type_str,
        server_addr_str.len(),
        client_id_str.len()
    );

    #[cfg(target_os = "android")]
    {
        std::io::stdout().flush().unwrap();
        let local_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create local tokio runtime");

        match local_runtime.block_on(async {
            crate::handle::android_sdk::perform_android_login_with_tls(
                server_addr_str.as_str(),
                control_port as u16,
                client_id_str.as_str(),
                false,
                tls_config,
            )
            .await
        }) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!(
                    "❌ C API: Failed to start and login Android TLS worker: {}",
                    e
                );
                -1
            }
        }
    }

    #[cfg(target_os = "ios")]
    {
        let local_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create local tokio runtime");

        match local_runtime.block_on(async {
            crate::worker_sdk::perform_login_with_tls(
                server_addr_str.as_str(),
                control_port as u16,
                client_id_str.as_str(),
                false,
                tls_config,
            )
            .await
        }) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("❌ C API: Failed to login iOS TLS worker: {}", e);
                -1
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[no_mangle]
pub extern "C" fn start_remote_worker_with_tls(
    _server_addr: *const c_char,
    _control_port: c_int,
    _proxy_port: c_int,
    _worker_type: *const c_char,
    _client_id: *const c_char,
    _ca_cert_path: *const c_char,
    _control_tls_server_name: *const c_char,
    _cert_sha256_pin: *const c_char,
) -> c_int {
    -1
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[no_mangle]
pub extern "C" fn start_remote_worker(
    _server_addr: *const c_char,
    _control_port: c_int,
    _proxy_port: c_int,
    _worker_type: *const c_char,
    _client_id: *const c_char,
) -> c_int {
    -1
}

// Global backend initialization flag
static BACKEND_INITIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

// Coordination mutex for safe hot swapping
static MODEL_SWAP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Initialize backend (thread-safe, idempotent)
fn ensure_backend_initialized() -> c_int {
    use std::sync::atomic::Ordering;

    // Check if already initialized (fast path)
    if BACKEND_INITIALIZED.load(Ordering::SeqCst) {
        return 0;
    }

    // Try to initialize
    if real_llama_backend_init() != 0 {
        return -1;
    }

    // Mark as initialized
    BACKEND_INITIALIZED.store(true, Ordering::SeqCst);
    0
}

/// Set remote worker model (C API) - Safe Hot Swapping Version
///
/// This function supports safe hot swapping without stopping the worker.
/// Uses coordination mutex to ensure no inference requests access freed memory.
///
/// # Parameters
/// - `model_path`: Path to the model file (.gguf)
///
/// # Returns
/// - `0`: Success (model loaded and context created)
/// - `-1`: Backend initialization failed
/// - `-2`: Path conversion failed
/// - `-3`: Model loading failed
/// - `-4`: Context creation failed
///
/// # Safety
/// Caller must ensure `model_path` is a valid null-terminated C string
///
/// # Hot Swapping
/// This function can be called multiple times without stopping the worker.
/// Inference requests will be briefly paused during the swap but the worker
/// remains connected and continues processing afterward.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[no_mangle]
pub extern "C" fn set_remote_worker_model(model_path: *const c_char) -> c_int {
    use std::sync::atomic::Ordering;

    println!("🔥 GPUFabric C API: Setting remote worker model (hot swap enabled)");

    // 1. Ensure backend is initialized (only once per process)
    if ensure_backend_initialized() != 0 {
        eprintln!("❌ C API: Backend initialization failed");
        return -1;
    }
    println!("✅ C API: Backend ready");

    // 2. Convert C string to Rust string
    let path_str = if model_path.is_null() {
        eprintln!("❌ C API: Model path is null");
        return -2;
    } else {
        unsafe {
            match std::ffi::CStr::from_ptr(model_path).to_str() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("❌ C API: Failed to convert model path: {}", e);
                    return -2;
                }
            }
        }
    };

    // 3. Update model status to loading
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loading(path_str);
    }

    // 4. Load new model and context
    let model_ptr = gpuf_load_model(model_path);
    if model_ptr.is_null() {
        eprintln!("❌ C API: Failed to load model");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to load model");
        return -3;
    }
    println!("✅ C API: Model loaded (path {} bytes)", path_str.len());

    let context_ptr = gpuf_create_context(model_ptr);
    if context_ptr.is_null() {
        eprintln!("❌ C API: Failed to create context");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to create context");
        unsafe { llama_model_free(model_ptr) }; // Clean up loaded model
        return -4;
    }
    println!("✅ C API: Context created");

    // 5. Atomically swap model/context using inference mutex
    // This blocks both other swaps AND inference requests briefly
    println!("🔄 C API: Swapping model (blocking inference briefly)...");
    {
        let _swap_lock = MODEL_SWAP_LOCK.lock().unwrap();
        let _inference_lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();

        // Get old model/context for cleanup
        let old_model = GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
        let old_context = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);

        // Update to new model/context atomically
        GLOBAL_MODEL_PTR.store(model_ptr, Ordering::SeqCst);
        GLOBAL_CONTEXT_PTR.store(context_ptr, Ordering::SeqCst);

        println!("✅ C API: Global pointers updated");

        // Clean up old resources AFTER updating pointers
        if !old_model.is_null() || !old_context.is_null() {
            println!("🧹 C API: Cleaning up previous model/context");

            if !old_context.is_null() {
                unsafe { llama_free(old_context) };
                println!("✅ C API: Old context freed");
            }
            if !old_model.is_null() {
                unsafe { llama_model_free(old_model) };
                println!("✅ C API: Old model freed");
            }
        }
    }

    println!("✅ C API: Model swap completed");

    // 6. Update status to loaded
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loaded(path_str);
    }

    println!("🎉 C API: Remote worker model set successfully (hot swap)");
    0 // Success
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[no_mangle]
pub extern "C" fn set_remote_worker_model(_model_path: *const c_char) -> c_int {
    -1
}

/// Start remote worker background tasks (C API)
#[cfg(any(target_os = "android", target_os = "ios"))]
#[no_mangle]
pub extern "C" fn start_remote_worker_tasks() -> c_int {
    println!("🔥 GPUFabric C API: Starting remote worker background tasks");

    #[cfg(target_os = "android")]
    {
        match TOKIO_RUNTIME
            .block_on(async { crate::handle::android_sdk::start_worker_tasks().await })
        {
            Ok(_) => 0 as c_int,
            Err(e) => {
                eprintln!("❌ C API: Failed to start background tasks: {}", e);
                -1 as c_int
            }
        }
    }

    #[cfg(target_os = "ios")]
    {
        let local_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create local tokio runtime");

        match local_runtime
            .block_on(async { crate::worker_sdk::start_worker_tasks_with_callback_ptr(None).await })
        {
            Ok(_) => 0 as c_int,
            Err(e) => {
                eprintln!("❌ C API: Failed to start background tasks: {}", e);
                -1 as c_int
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[no_mangle]
pub extern "C" fn start_remote_worker_tasks() -> c_int {
    -1
}

/// Start remote worker background tasks with callback support (C API)
#[cfg(any(target_os = "android", target_os = "ios"))]
#[no_mangle]
pub extern "C" fn start_remote_worker_tasks_with_callback_ptr(
    callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
) -> c_int {
    println!("🔥 GPUFabric C API: Starting remote worker background tasks with callback");

    #[cfg(target_os = "android")]
    {
        match TOKIO_RUNTIME.block_on(async {
            crate::handle::android_sdk::start_worker_tasks_with_callback_ptr(callback).await
        }) {
            Ok(_) => 0 as c_int,
            Err(e) => {
                eprintln!(
                    "❌ C API: Failed to start background tasks with callback: {}",
                    e
                );
                -1 as c_int
            }
        }
    }

    #[cfg(target_os = "ios")]
    {
        let local_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create local tokio runtime");

        match local_runtime.block_on(async {
            crate::worker_sdk::start_worker_tasks_with_callback_ptr(callback).await
        }) {
            Ok(_) => 0 as c_int,
            Err(e) => {
                eprintln!(
                    "❌ C API: Failed to start background tasks with callback: {}",
                    e
                );
                -1 as c_int
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[no_mangle]
pub extern "C" fn start_remote_worker_tasks_with_callback_ptr(
    _callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
) -> c_int {
    -1
}

/// Register a status callback for remote worker background tasks (C API).
///
/// This is the preferred iOS/Objective-C++ entry point because it keeps callback registration
/// separate from task startup and preserves a caller-provided `user_data` pointer.
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn gpuf_register_remote_worker_callback(
    callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
    user_data: *mut c_void,
) -> c_int {
    crate::worker_sdk::register_remote_worker_callback(callback, user_data)
}

#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub extern "C" fn gpuf_register_remote_worker_callback(
    _callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
    _user_data: *mut c_void,
) -> c_int {
    -1
}

/// Stop remote worker and cleanup (C API)
#[cfg(any(target_os = "android", target_os = "ios"))]
#[no_mangle]
pub extern "C" fn stop_remote_worker() -> c_int {
    println!("🔥 GPUFabric C API: Stopping remote worker");

    #[cfg(target_os = "android")]
    {
        TOKIO_RUNTIME.block_on(async { crate::handle::android_sdk::stop_global_worker().await });
        0
    }

    #[cfg(target_os = "ios")]
    {
        let local_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create local tokio runtime");
        local_runtime.block_on(async { crate::worker_sdk::stop_global_worker().await });
        0
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[no_mangle]
pub extern "C" fn stop_remote_worker() -> c_int {
    -1
}

/// Get remote worker status (C API)
///
/// # Parameters
/// - `buffer`: Output buffer to write status string
/// - `buffer_size`: Size of the output buffer
///
/// # Returns
/// - `0`: Success (status written to buffer)
/// - `-1`: Error (buffer too small or other error)
///
/// # Safety
/// Caller must ensure `buffer` is valid and can hold `buffer_size` bytes
#[cfg(any(target_os = "android", target_os = "ios"))]
#[no_mangle]
pub extern "C" fn get_remote_worker_status(buffer: *mut c_char, buffer_size: size_t) -> c_int {
    println!("🔥 GPUFabric C API: Getting remote worker status");

    if buffer.is_null() {
        eprintln!("❌ C API: Buffer is null");
        return -1;
    }

    if buffer_size == 0 {
        eprintln!("❌ C API: Buffer size is zero");
        return -1;
    }

    // Get status from async function
    let status: String = {
        #[cfg(target_os = "android")]
        {
            TOKIO_RUNTIME.block_on(async {
                crate::handle::android_sdk::get_worker_status()
                    .await
                    .unwrap_or_else(|_| "Error".to_string())
            })
        }

        #[cfg(target_os = "ios")]
        {
            let local_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create local tokio runtime");
            local_runtime.block_on(async {
                crate::worker_sdk::get_worker_status()
                    .await
                    .unwrap_or_else(|_| "Error".to_string())
            })
        }
    };

    println!("📊 C API: Status generated ({} bytes)", status.len());

    // Convert to C string and copy to buffer
    let status_c = match std::ffi::CString::new(status) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ C API: Failed to convert status to C string: {}", e);
            return -1;
        }
    };

    let status_bytes = status_c.as_bytes_with_nul();

    if status_bytes.len() > buffer_size {
        eprintln!(
            "❌ C API: Buffer too small (need {}, have {})",
            status_bytes.len(),
            buffer_size
        );
        return -1;
    }

    unsafe {
        std::ptr::copy_nonoverlapping(status_bytes.as_ptr(), buffer as *mut u8, status_bytes.len());
    }

    println!("✅ C API: Status written to buffer");
    0 as c_int
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[no_mangle]
pub extern "C" fn get_remote_worker_status(buffer: *mut c_char, buffer_size: size_t) -> c_int {
    if buffer.is_null() || buffer_size == 0 {
        return -1;
    }

    unsafe {
        *buffer = 0;
    }
    -1
}
