use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

// JNI imports for Android
#[cfg(target_os = "android")]
use jni::JNIEnv;

#[cfg(target_os = "android")]
use jni::objects::{JClass, JString, JObject};

#[cfg(target_os = "android")]
use jni::sys::{jstring, jlong, jint};

// Export modules
pub mod llm_engine;
pub mod util;

// Simulate llama.cpp structs (avoid C++ symbol dependencies)
#[repr(C)]
pub struct llama_model {
    _private: [u8; 0],
}

#[repr(C)]
pub struct llama_context {
    _private: [u8; 0],
}

#[repr(C)]
pub struct llama_model_params {
    pub n_gpu_layers: i32,
    pub main_gpu: i32,
    pub tensor_split: *const f32,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub progress_callback: Option<extern "C" fn(f32, *mut c_void)>,
    pub progress_callback_user_data: *mut c_void,
    pub kv_overrides: *const c_char,
    pub vocab_only: bool,
}

#[repr(C)]
pub struct llama_context_params {
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_gpu_layers: i32,
    pub main_gpu: i32,
    pub tensor_split: *const f32,
    pub f16_kv: bool,
    pub logits_all: bool,
    pub embedding: bool,
    pub offload_kqv: bool,
    pub rope_scaling_type: i32,
    pub rope_freq_base: f32,
    pub rope_freq_scale: f32,
    pub yarn_ext_factor: f32,
    pub yarn_attn_factor: f32,
    pub yarn_beta_fast: f32,
    pub yarn_beta_slow: f32,
    pub yarn_orig_ctx: i32,
    pub pooling_type: i32,
}

pub type LlamaToken = i32;

// ============================================================================
// Real llama.cpp API Functions (for Android)
// ============================================================================

#[cfg(target_os = "android")]
extern "C" {
    // Backend functions
    fn llama_backend_init() -> c_int;
    fn llama_backend_free();
    fn llama_load_model_from_file(path: *const c_char, params: llama_model_params) -> *mut llama_model;
    fn llama_init_from_model(model: *const llama_model, params: llama_context_params) -> *mut llama_context;
    fn llama_tokenize(ctx: *mut llama_context, text: *const c_char, tokens: *mut LlamaToken, n_max_tokens: c_int, add_bos: bool) -> c_int;
    
    // Generation functions
    fn llama_generate(
        ctx: *mut llama_context,
        tokens: *const LlamaToken,
        n_tokens: c_int,
        n_past: *mut c_int,
        n_threads: c_int,
    ) -> c_int;
    
    // Utility functions
    fn llama_n_ctx(ctx: *const llama_context) -> c_int;
    fn llama_model_n_vocab(model: *const llama_model) -> c_int;
    fn llama_token_bos(model: *const llama_model) -> LlamaToken;
    fn llama_token_eos(model: *const llama_model) -> LlamaToken;
    
    // Memory management functions
    fn llama_model_free(model: *mut llama_model);
    fn llama_free(ctx: *mut llama_context);
    
    // GGML backend functions - force linking
    fn ggml_backend_dev_by_type(type_: i32) -> *mut ();
    fn ggml_backend_dev_get(i: i32) -> *mut ();
    fn ggml_backend_dev_count() -> i32;
}

// ============================================================================
// Real llama.cpp API Wrappers
// ============================================================================

#[cfg(target_os = "android")]
fn real_llama_backend_init() -> c_int {
    unsafe { llama_backend_init() }
}

#[cfg(target_os = "android")]
fn real_llama_backend_free() {
    unsafe { llama_backend_free() }
}

#[cfg(target_os = "android")]
fn real_llama_model_load_from_file(path: *const c_char, params: llama_model_params) -> *mut llama_model {
    unsafe { llama_load_model_from_file(path, params) }
}

#[cfg(target_os = "android")]
#[allow(dead_code)]
fn real_llama_model_free(model: *mut llama_model) {
    unsafe { llama_model_free(model) }
}

#[cfg(target_os = "android")]
fn real_llama_init_from_model(model: *const llama_model, params: llama_context_params) -> *mut llama_context {
    unsafe { llama_init_from_model(model, params) }
}

#[cfg(target_os = "android")]
#[allow(dead_code)]
fn real_llama_free(ctx: *mut llama_context) {
    unsafe { llama_free(ctx) }
}

#[cfg(target_os = "android")]
fn real_llama_tokenize(
    ctx: *mut llama_context,
    text: *const c_char,
    tokens: *mut LlamaToken,
    n_max_tokens: c_int,
    add_bos: bool,
) -> c_int {
    unsafe { llama_tokenize(ctx, text, tokens, n_max_tokens, add_bos) }
}

#[cfg(target_os = "android")]
fn real_llama_n_ctx(ctx: *const llama_context) -> c_int {
    unsafe { llama_n_ctx(ctx) }
}

// ============================================================================
// Non-Android (fallback to simulation)
// ============================================================================

#[cfg(not(target_os = "android"))]
fn real_llama_backend_init() -> c_int {
    simulate_llama_backend_init()
}

#[cfg(not(target_os = "android"))]
fn real_llama_backend_free() {
    simulate_llama_backend_free()
}

#[cfg(not(target_os = "android"))]
fn real_llama_model_load_from_file(path: *const c_char, params: llama_model_params) -> *mut llama_model {
    simulate_llama_model_load_from_file(path, params)
}

#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
fn real_llama_model_free(model: *mut llama_model) {
    simulate_llama_model_free(model)
}

#[cfg(not(target_os = "android"))]
fn real_llama_init_from_model(model: *const llama_model, params: llama_context_params) -> *mut llama_context {
    simulate_llama_init_from_model(model, params)
}

#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
fn real_llama_free(ctx: *mut llama_context) {
    simulate_llama_free(ctx)
}

#[cfg(not(target_os = "android"))]
fn real_llama_tokenize(
    ctx: *mut llama_context,
    text: *const c_char,
    tokens: *mut LlamaToken,
    n_max_tokens: c_int,
    add_bos: bool,
) -> c_int {
    simulate_llama_tokenize(ctx, text, tokens, n_max_tokens, add_bos)
}

#[cfg(not(target_os = "android"))]
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

fn simulate_llama_model_load_from_file(path: *const c_char, _params: llama_model_params) -> *mut llama_model {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    
    let path_str = unsafe {
        CStr::from_ptr(path).to_str().unwrap_or("invalid_path")
    };
    
    println!("🔧 Simulating llama_load_model_from_file({})", path_str);
    std::ptr::NonNull::dangling().as_ptr()
}

#[allow(dead_code)]
fn simulate_llama_model_free(model: *mut llama_model) {
    if !model.is_null() {
        println!("🧹 Simulating llama_model_free()");
    }
}

fn simulate_llama_init_from_model(model: *const llama_model, _params: llama_context_params) -> *mut llama_context {
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
    
    let text_str = unsafe {
        CStr::from_ptr(text).to_str().unwrap_or("")
    };
    
    println!("🔧 Simulating llama_tokenize({})", text_str);
    
    // Return fake token count
    let token_count = text_str.len().min(n_max_tokens as usize);
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
        n_gpu_layers: 0,
        main_gpu: 0,
        tensor_split: std::ptr::null(),
        use_mmap: true,
        use_mlock: false,
        progress_callback: None,
        progress_callback_user_data: std::ptr::null_mut(),
        kv_overrides: std::ptr::null(),
        vocab_only: false,
    }
}

fn simulate_llama_context_default_params() -> llama_context_params {
    llama_context_params {
        n_ctx: 2048,
        n_batch: 512,
        n_gpu_layers: 0,
        main_gpu: 0,
        tensor_split: std::ptr::null(),
        f16_kv: true,
        logits_all: false,
        embedding: false,
        offload_kqv: false,
        rope_scaling_type: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
        yarn_ext_factor: -1.0,
        yarn_attn_factor: 1.0,
        yarn_beta_fast: 32.0,
        yarn_beta_slow: 1.0,
        yarn_orig_ctx: 0,
        pooling_type: 0,
    }
}

// Final solution: Use real llama.cpp API on Android, simulated on other platforms

#[no_mangle]
pub extern "C" fn gpuf_load_model(path: *const c_char) -> *mut llama_model {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    
    real_llama_backend_init();
    let params = simulate_llama_model_default_params();
    real_llama_model_load_from_file(path, params)
}

#[no_mangle]
pub extern "C" fn gpuf_create_context(model: *mut llama_model) -> *mut llama_context {
    if model.is_null() {
        return std::ptr::null_mut();
    }
    
    let params = simulate_llama_context_default_params();
    real_llama_init_from_model(model, params)
}

#[no_mangle]
pub extern "C" fn gpuf_tokenize_text(
    ctx: *mut llama_context,
    text: *const c_char,
    tokens: *mut LlamaToken,
    max_tokens: c_int,
) -> c_int {
    if ctx.is_null() || text.is_null() || tokens.is_null() {
        return -1;
    }
    real_llama_tokenize(ctx, text, tokens, max_tokens, true)
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
        let mut tokens = vec![0 as LlamaToken; 1024];
        let token_count = real_llama_tokenize(ctx, prompt, tokens.as_mut_ptr(), 1024, true);
        
        let n_ctx = real_llama_n_ctx(ctx);
        
        // Simple output for demonstration
        let output_text = format!("Generated: {} (tokens: {}, ctx: {})", prompt_str, token_count, n_ctx);
        let output_cstr = CString::new(output_text).unwrap();
        
        let copy_len = std::cmp::min(output_cstr.as_bytes().len(), output_len as usize);
        std::ptr::copy_nonoverlapping(output_cstr.as_ptr(), output, copy_len);
        *output.add(copy_len) = 0;
        
        copy_len as c_int
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
        // Note: C++ runtime is still dynamically linked for compatibility
        // OpenMP and llama.cpp are statically linked
        use std::env;
        
        if env::var("LD_PRELOAD").is_err() {
            let possible_paths = vec![
                "/system/lib64/libc++_shared.so",                    // Standard ARM64
                "/system/lib/libc++_shared.so",                      // Standard ARM32
                "/apex/com.android.runtime/lib64/libc++_shared.so",  // APEX ARM64
                "/apex/com.android.runtime/lib/libc++_shared.so",    // APEX ARM32
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
                    println!("⚠️  Warning: libc++_shared.so not found in standard locations");
                    println!("   C++ runtime may not be available - manual LD_PRELOAD may be needed");
                }
            }
        } else {
            println!("✅ LD_PRELOAD already set: {}", env::var("LD_PRELOAD").unwrap_or_default());
        }
        
        real_llama_backend_init();
        
        // Force reference to GGML backend symbols to ensure they are linked
        unsafe {
            // These symbols must be available at runtime
            let _ggml_backend_dev_by_type_ptr = ggml_backend_dev_by_type as *const ();
            let _ggml_backend_dev_get_ptr = ggml_backend_dev_get as *const ();
            let _ggml_backend_dev_count_ptr = ggml_backend_dev_count as *const ();
            
            // Prevent compiler from optimizing away the symbol references
            std::hint::black_box(_ggml_backend_dev_by_type_ptr);
            std::hint::black_box(_ggml_backend_dev_get_ptr);
            std::hint::black_box(_ggml_backend_dev_count_ptr);
        }
    }
    
    #[cfg(not(target_os = "android"))]
    {
        real_llama_backend_init();
    }
    
    0
}

#[no_mangle]
pub extern "C" fn gpuf_cleanup() -> c_int {
    println!("🧹 GPUFabric Android LLaMA.cpp solution cleaned up");
    real_llama_backend_free();
    0
}

// ============================================================================
// JNI API Functions for Android
// ============================================================================

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_initialize(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    println!("🔥 GPUFabric JNI: Initializing engine");
    match gpuf_init() {
        0 => 1, // Success
        _ => 0, // Failure
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_loadModel(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) -> jlong {
    println!("🔥 GPUFabric JNI: Loading model");
    
    let path = match env.get_string(&model_path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    
    let path_str = match path.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    
    let path_cstr = match CString::new(path_str) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    
    let model_ptr = gpuf_load_model(path_cstr.as_ptr());
    model_ptr as jlong
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_createContext(
    _env: JNIEnv,
    _class: JClass,
    model_ptr: jlong,
) -> jlong {
    println!("🔥 GPUFabric JNI: Creating context");
    
    if model_ptr == 0 {
        return 0;
    }
    
    let context_ptr = gpuf_create_context(model_ptr as *mut llama_model);
    context_ptr as jlong
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_generate(
    mut env: JNIEnv,
    _class: JClass,
    model_ptr: jlong,
    context_ptr: jlong,
    prompt: JString,
    max_tokens: jint,
    _output_buffer: JObject,
) -> jint {
    println!("🔥 GPUFabric JNI: Generating text");
    
    if model_ptr == 0 || context_ptr == 0 {
        return -1;
    }
    
    let prompt_str = match env.get_string(&prompt) {
        Ok(s) => s,
        Err(_) => return -2,
    };
    
    let prompt_cstr = match CString::new(prompt_str.to_str().unwrap_or("")) {
        Ok(s) => s,
        Err(_) => return -3,
    };
    
    // Create a buffer for output (simplified version)
    let mut output = vec![0u8; 4096];
    
    let result = gpuf_generate_final_solution_text(
        model_ptr as *mut llama_model,
        context_ptr as *mut llama_context,
        prompt_cstr.as_ptr(),
        max_tokens as i32,
        output.as_mut_ptr() as *mut c_char,
        output.len() as i32,
    );
    
    if result > 0 {
        // Convert output to Java string (simplified)
        let _output_str = unsafe {
            CStr::from_ptr(output.as_mut_ptr() as *const c_char)
                .to_str()
                .unwrap_or("")
        };
        
        // Set the output buffer content (this is simplified, proper implementation needed)
        // In real implementation, you would set the Java string buffer content
        
        result
    } else {
        result
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_getVersion(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    println!("🔥 GPUFabric JNI: Getting version");
    
    let version_ptr = gpuf_version();
    if version_ptr.is_null() {
        return std::ptr::null_mut();
    }
    
    let version_str = unsafe {
        CStr::from_ptr(version_ptr)
            .to_str()
            .unwrap_or("unknown")
    };
    
    env.new_string(version_str).unwrap_or_else(|_| unsafe { JString::from_raw(std::ptr::null_mut()) }).into_raw()
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_cleanup(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    println!("🔥 GPUFabric JNI: Cleaning up");
    match gpuf_cleanup() {
        0 => 1, // Success
        _ => 0, // Failure
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_getSystemInfo(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    println!("🔥 GPUFabric JNI: Getting system info");
    
    let info_cstr = gpuf_system_info();
    if info_cstr.is_null() {
        return std::ptr::null_mut();
    }
    
    let info_str = unsafe {
        CStr::from_ptr(info_cstr).to_str().unwrap_or("Unknown")
    };
    
    match env.new_string(info_str) {
        Ok(jstring) => jstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_gpuf_1init(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    println!("🔥 GPUFabric JNI: Calling gpuf_init");
    
    match gpuf_init() {
        0 => 0, // Success
        error_code => error_code as jint, // Return actual error code
    }
}
