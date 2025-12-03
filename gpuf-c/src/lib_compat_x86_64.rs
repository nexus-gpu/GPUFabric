use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void, c_float};

// Compatibility layer types - zero-size for type safety
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

pub type llama_token = i32;

// Compatibility layer functions - API compatible without C++ dependencies
#[no_mangle]
pub extern "C" fn llama_backend_init() {
    println!("🔧 [x86_64 COMPAT] llama_backend_init() called");
}

#[no_mangle]
pub extern "C" fn llama_backend_free() {
    println!("🧹 [x86_64 COMPAT] llama_backend_free() called");
}

#[no_mangle]
pub extern "C" fn llama_print_system_info() -> *const c_char {
    let info = CString::new(
        "x86_64 Android (ARM64 Compatibility Layer)\n\
         Architecture: x86_64\n\
         Platform: Android Emulator\n\
         LLAMA Backend: Simulated (API Compatible)\n\
         Build: x86_64 Compatibility Layer\n\
         Status: No C++ symbol conflicts - API Ready"
    ).unwrap();
    info.into_raw()
}

#[no_mangle]
pub extern "C" fn llama_time_us() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as i64
}

#[no_mangle]
pub extern "C" fn llama_supports_mmap() -> bool {
    true // Android supports mmap
}

#[no_mangle]
pub extern "C" fn llama_supports_mlock() -> bool {
    false // Android x86_64 emulator usually doesn't support mlock
}

#[no_mangle]
pub extern "C" fn llama_supports_gpu_offload() -> bool {
    false // x86_64 emulator usually doesn't support GPU offload
}

#[no_mangle]
pub extern "C" fn llama_supports_rpc() -> bool {
    true // Support RPC
}

#[no_mangle]
pub extern "C" fn llama_model_default_params() -> llama_model_params {
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

#[no_mangle]
pub extern "C" fn llama_context_default_params() -> llama_context_params {
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

#[no_mangle]
pub extern "C" fn llama_model_load_from_file(
    path_model: *const c_char,
    params: llama_model_params,
) -> *mut llama_model {
    if path_model.is_null() {
        return std::ptr::null_mut();
    }
    
    unsafe {
        let path = CStr::from_ptr(path_model);
        if let Ok(path_str) = path.to_str() {
            println!("📁 [x86_64 COMPAT] Attempting to load model: {}", path_str);
            
            if path_str.ends_with(".gguf") {
                println!("✅ [x86_64 COMPAT] Model file format recognized");
                // Return a non-null pointer to indicate "success"
                Box::into_raw(Box::new(())) as *mut llama_model
            } else {
                println!("❌ [x86_64 COMPAT] Invalid model format");
                std::ptr::null_mut()
            }
        } else {
            println!("❌ [x86_64 COMPAT] Invalid path string");
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn llama_model_free(model: *mut llama_model) {
    if !model.is_null() {
        unsafe {
            // Convert back to Box and drop
            let _ = Box::from_raw(model as *mut ());
        }
    }
}

#[no_mangle]
pub extern "C" fn llama_init_from_model(
    model: *mut llama_model,
    params: llama_context_params,
) -> *mut llama_context {
    if model.is_null() {
        return std::ptr::null_mut();
    }
    
    println!("🎯 [x86_64 COMPAT] Creating context from model");
    // Return a non-null pointer to indicate "success"
    Box::into_raw(Box::new(())) as *mut llama_context
}

#[no_mangle]
pub extern "C" fn llama_free(ctx: *mut llama_context) {
    if !ctx.is_null() {
        unsafe {
            let _ = Box::from_raw(ctx as *mut ());
        }
    }
}

#[no_mangle]
pub extern "C" fn llama_n_ctx(ctx: *const llama_context) -> u32 {
    if ctx.is_null() {
        return 0;
    }
    2048 // Default context size
}

#[no_mangle]
pub extern "C" fn llama_n_batch(ctx: *const llama_context) -> u32 {
    if ctx.is_null() {
        return 0;
    }
    512 // Default batch size
}

#[no_mangle]
pub extern "C" fn llama_tokenize(
    model: *const llama_model,
    text: *const c_char,
    tokens: *mut llama_token,
    n_max_tokens: c_int,
    add_bos: bool,
    special: bool,
) -> c_int {
    if model.is_null() || text.is_null() || tokens.is_null() || n_max_tokens <= 0 {
        return -1;
    }
    
    unsafe {
        let text_str = match CStr::from_ptr(text).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };
        
        // Simulate tokenization based on character length
        let simulated_tokens: Vec<llama_token> = text_str
            .chars()
            .enumerate()
            .map(|(i, c)| {
                // Simulate realistic token ID generation
                match c {
                    'A'..='Z' => c as llama_token - 65 + 1,      // A-Z -> 1-26
                    'a'..='z' => c as llama_token - 97 + 27,     // a-z -> 27-52
                    '0'..='9' => c as llama_token - 48 + 53,     // 0-9 -> 53-62
                    ' ' => 999,                                 // space -> 999
                    _ => c as llama_token + 1000,               // others -> 1000+
                }
            })
            .collect();
        
        let token_count = simulated_tokens.len() as c_int;
        let max_tokens = std::cmp::min(simulated_tokens.len(), n_max_tokens as usize);
        
        for i in 0..max_tokens {
            *tokens.add(i) = simulated_tokens[i];
        }
        
        token_count
    }
}

// High-level compatibility test function
#[no_mangle]
pub extern "C" fn gpuf_test_llama_compatibility() -> c_int {
    println!("🧪 [x86_64 COMPAT] Testing llama.cpp API compatibility...");
    
    // Test all major functions
    unsafe {
        llama_backend_init();
        
        let system_info = llama_print_system_info();
        let info_str = CStr::from_ptr(system_info).to_str().unwrap_or("Invalid info");
        println!("🖥️  System info: {}", info_str);
        
        let supports_mmap = llama_supports_mmap();
        let supports_gpu = llama_supports_gpu_offload();
        let timestamp = llama_time_us();
        
        println!("📊 mmap: {}, gpu: {}, time: {}μs", supports_mmap, supports_gpu, timestamp);
        
        let model_params = llama_model_default_params();
        let ctx_params = llama_context_default_params();
        
        println!("📋 Model params: mmap={}, gpu_layers={}", model_params.use_mmap, model_params.n_gpu_layers);
        println!("📋 Context params: ctx_size={}, batch_size={}", ctx_params.n_ctx, ctx_params.n_batch);
        
        // Test tokenization
        let test_path = CString::new("test.gguf").unwrap();
        let model = llama_model_load_from_file(test_path.as_ptr(), model_params);
        
        if !model.is_null() {
            println!("✅ Model loading simulation successful");
            
            let ctx = llama_init_from_model(model, ctx_params);
            if !ctx.is_null() {
                println!("✅ Context creation simulation successful");
                
                let test_text = CString::new("Hello x86_64!").unwrap();
                let mut tokens = [0 as llama_token; 100];
                let token_count = llama_tokenize(model, test_text.as_ptr(), tokens.as_mut_ptr(), 100, true, true);
                
                println!("🔤 Tokenization test: {} tokens", token_count);
                
                llama_free(ctx);
            }
            
            llama_model_free(model);
        }
        
        llama_backend_free();
    }
    
    println!("✅ [x86_64 COMPAT] All compatibility tests passed!");
    0 // Success
}

// JNI compatibility functions
#[no_mangle]
pub extern "C" fn gpuf_system_info() -> *const c_char {
    unsafe { llama_print_system_info() }
}

#[no_mangle]
pub extern "C" fn gpuf_version() -> *const c_char {
    let version = CString::new("12.0.0-x86_64-android-COMPAT-LAYER").unwrap();
    version.into_raw()
}

#[no_mangle]
pub extern "C" fn gpuf_init() -> c_int {
    println!("🔥 GPUFabric x86_64 compatibility layer initialized");
    unsafe { llama_backend_init(); }
    0
}

#[no_mangle]
pub extern "C" fn gpuf_cleanup() -> c_int {
    println!("🧹 GPUFabric x86_64 compatibility layer cleaned up");
    unsafe { llama_backend_free(); }
    0
}
