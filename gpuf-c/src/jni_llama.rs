// ============================================================================
// JNI Wrappers for Local LLaMA Inference API
// ============================================================================
//
// This module provides JNI bindings for local LLaMA model inference,
// allowing Android applications to run LLM inference directly on device.
//
// Package: com.gpuf.c.GPUEngine
// ============================================================================

#[cfg(target_os = "android")]
use jni::objects::{JClass, JObject, JString};
#[cfg(target_os = "android")]
use jni::sys::{jboolean, jbyteArray, jfloat, jint, jlong, jstring};
#[cfg(target_os = "android")]
use jni::JNIEnv;

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::Ordering;

use crate::{
    gpuf_cleanup, gpuf_create_context, gpuf_create_multimodal_context, gpuf_free_multimodal_model,
    gpuf_generate_final_solution_text, gpuf_generate_multimodal, gpuf_get_model_status, gpuf_init,
    gpuf_is_context_ready, gpuf_is_model_loaded, gpuf_load_model, gpuf_load_model_async,
    gpuf_load_multimodal_model, gpuf_multimodal_model, gpuf_multimodal_supports_vision,
    gpuf_start_generation_async, gpuf_stop_generation, gpuf_system_info, gpuf_version,
    llama_context, llama_model, manual_llama_completion, should_stop_generation,
    GLOBAL_CONTEXT_PTR, GLOBAL_MODEL_PTR, MODEL_STATUS,
};

// ============================================================================
// Basic Engine Management
// ============================================================================

/// Initialize the GPUFabric engine
///
/// Java signature:
/// public static native int initialize();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_initialize(_env: JNIEnv, _class: JClass) -> jint {
    println!("🔥 GPUFabric JNI: Initializing engine");
    match gpuf_init() {
        0 => 1, // Success
        _ => 0, // Failure
    }
}

/// Get GPUFabric version string
///
/// Java signature:
/// public static native String getVersion();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_getVersion(env: JNIEnv, _class: JClass) -> jstring {
    println!("🔥 GPUFabric JNI: Getting version");

    let version_ptr = gpuf_version();
    if version_ptr.is_null() {
        return std::ptr::null_mut();
    }

    let version_str = unsafe { CStr::from_ptr(version_ptr).to_str().unwrap_or("unknown") };

    env.new_string(version_str)
        .unwrap_or_else(|_| unsafe { JString::from_raw(std::ptr::null_mut()) })
        .into_raw()
}

/// Cleanup and free resources
///
/// Java signature:
/// public static native int cleanup();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_cleanup(_env: JNIEnv, _class: JClass) -> jint {
    println!("🔥 GPUFabric JNI: Cleaning up");
    match gpuf_cleanup() {
        0 => 1, // Success
        _ => 0, // Failure
    }
}

/// Get system information
///
/// Java signature:
/// public static native String getSystemInfo();
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

    let info_str = unsafe { CStr::from_ptr(info_cstr).to_str().unwrap_or("Unknown") };

    match env.new_string(info_str) {
        Ok(jstring) => jstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// Model Loading and Management
// ============================================================================

/// Load a LLaMA model from file
///
/// Java signature:
/// public static native long loadModel(String modelPath);
///
/// Returns: model pointer as long, or 0 on failure
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

/// Create inference context for a model
///
/// Java signature:
/// public static native long createContext(long modelPtr);
///
/// Returns: context pointer as long, or 0 on failure
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

/// Check if model is loaded
///
/// Java signature:
/// public static native boolean isModelLoaded();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_isModelLoaded(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if gpuf_is_model_loaded() {
        1 // true
    } else {
        0 // false
    }
}

/// Check if context is ready
///
/// Java signature:
/// public static native boolean isContextReady();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_isContextReady(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if gpuf_is_context_ready() {
        1 // true
    } else {
        0 // false
    }
}

/// Get model loading status
///
/// Java signature:
/// public static native String getModelStatus();
///
/// Returns: "not_loaded", "loading", "ready", "error", or "unknown"
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_getModelStatus(env: JNIEnv, _class: JClass) -> jstring {
    let status_code = gpuf_get_model_status();
    let status_str = match status_code {
        0 => "not_loaded",
        1 => "loading",
        2 => "ready",
        3 => "error",
        _ => "unknown",
    };

    match env.new_string(status_str) {
        Ok(jstring) => jstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// Inference Service Management
// ============================================================================

/// Start inference service with model loading
///
/// Java signature:
/// public static native int startInferenceService(String modelPath, int port);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_startInferenceService(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
    _port: jint,
) -> jint {
    println!("🔥 GPUFabric JNI: Starting inference service");

    let path = match env.get_string(&model_path) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let path_str = match path.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Update model status
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loading(path_str);
    }

    // Load model
    let path_cstr = match CString::new(path_str) {
        Ok(s) => s,
        Err(_) => {
            let mut status = MODEL_STATUS.lock().unwrap();
            status.set_error("Failed to convert path to CString");
            return -2;
        }
    };

    let model_ptr = gpuf_load_model(path_cstr.as_ptr());
    if model_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Failed to load model");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to load model");
        return -3;
    }

    // Create context
    let context_ptr = gpuf_create_context(model_ptr);
    if context_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Failed to create context");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to create context");
        return -4;
    }

    // Store global pointers
    {
        GLOBAL_MODEL_PTR.store(model_ptr, Ordering::SeqCst);
        GLOBAL_CONTEXT_PTR.store(context_ptr, Ordering::SeqCst);
    }

    // Update status
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loaded(path_str);
    }

    println!("🔥 GPUFabric JNI: Inference service started successfully");
    1 // Success
}

/// Start inference service asynchronously with progress callback
///
/// Java signature:
/// public static native int startInferenceServiceAsync(String modelPath, int port, Object progressCallback);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_startInferenceServiceAsync(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
    _port: jint,
    progress_callback: JObject,
) -> jint {
    println!("🔥 GPUFabric JNI: Starting async inference service...");

    let path = match env.get_string(&model_path) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let path_str = match path.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Update model status to loading
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loading(path_str);
    }

    // Create global reference for progress callback
    let _progress_global = if progress_callback.is_null() {
        None
    } else {
        match env.new_global_ref(&progress_callback) {
            Ok(obj) => Some(obj),
            Err(e) => {
                println!(
                    "❌ JNI: Failed to create progress callback global ref: {:?}",
                    e
                );
                return -1;
            }
        }
    };

    let path_cstr = match CString::new(path_str) {
        Ok(s) => s,
        Err(_) => {
            let mut status = MODEL_STATUS.lock().unwrap();
            status.set_error("Failed to convert path to CString");
            return -2;
        }
    };

    // Define progress callback function
    extern "C" fn model_progress_callback(progress: f32, _user_data: *mut c_void) {
        if progress < 0.0 {
            println!("❌ Model loading failed!");
        } else if progress >= 1.0 {
            println!("✅ Model loading completed!");
        } else {
            println!("📊 Model loading progress: {:.1}%", progress * 100.0);
        }
    }

    // Start async model loading
    let model_ptr = gpuf_load_model_async(
        path_cstr.as_ptr(),
        Some(model_progress_callback),
        std::ptr::null_mut(),
    );

    if model_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Failed to load model");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to load model");
        return -3;
    }

    // Create context
    println!("🔧 Creating context (fast operation)...");
    let context_ptr = gpuf_create_context(model_ptr);

    if context_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Failed to create context");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to create context");
        return -4;
    }

    // Store global pointers
    {
        GLOBAL_MODEL_PTR.store(model_ptr, Ordering::SeqCst);
        GLOBAL_CONTEXT_PTR.store(context_ptr, Ordering::SeqCst);
    }

    // Update status to ready
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loaded(path_str);
    }

    println!("🔥 GPUFabric JNI: Async inference service started successfully");
    1 // Success
}

/// Stop inference service
///
/// Java signature:
/// public static native int stopInferenceService();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_stopInferenceService(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    println!("🔥 GPUFabric JNI: Stopping inference service");

    // Clear global pointers and status
    {
        GLOBAL_MODEL_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);
        GLOBAL_CONTEXT_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);
    }

    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.clear();
    }

    println!("🔥 GPUFabric JNI: Inference service stopped");
    1 // Success
}

/// Load model dynamically (alternative method)
///
/// Java signature:
/// public static native int loadModelNew(String modelPath);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_loadModelNew(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) -> jint {
    println!("🔥 GPUFabric JNI: Loading model dynamically");

    let path = match env.get_string(&model_path) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let path_str = match path.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Update model status
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loading(path_str);
    }

    // Load model
    let path_cstr = match CString::new(path_str) {
        Ok(s) => s,
        Err(_) => {
            let mut status = MODEL_STATUS.lock().unwrap();
            status.set_error("Failed to convert path to CString");
            return -2;
        }
    };

    let model_ptr = gpuf_load_model(path_cstr.as_ptr());
    if model_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Failed to load model");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to load model");
        return -3;
    }

    // Create context
    let context_ptr = gpuf_create_context(model_ptr);
    if context_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Failed to create context");
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_error("Failed to create context");
        return -4;
    }

    // Store global pointers
    {
        GLOBAL_MODEL_PTR.store(model_ptr, Ordering::SeqCst);
        GLOBAL_CONTEXT_PTR.store(context_ptr, Ordering::SeqCst);
    }

    // Update status
    {
        let mut status = MODEL_STATUS.lock().unwrap();
        status.set_loaded(path_str);
    }

    println!("🔥 GPUFabric JNI: Model loaded successfully");
    1 // Success
}

/// Get current loaded model path
///
/// Java signature:
/// public static native String getCurrentModel();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_getCurrentModel(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    println!("🔥 GPUFabric JNI: Getting current model");

    let status = MODEL_STATUS.lock().unwrap();
    match &status.current_model {
        Some(model) => match env.new_string(model) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

/// Get model loading status string
///
/// Java signature:
/// public static native String getModelLoadingStatus();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_getModelLoadingStatus(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    println!("🔥 GPUFabric JNI: Getting model loading status");

    let status = MODEL_STATUS.lock().unwrap();
    let status_str = if let Some(ref error) = status.error_message {
        format!("{}: {}", status.loading_status, error)
    } else {
        status.loading_status.clone()
    };

    match env.new_string(status_str) {
        Ok(jstring) => jstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// Text Generation
// ============================================================================

/// Generate text using loaded model (basic version)
///
/// Java signature:
/// public static native int generate(long modelPtr, long contextPtr, String prompt, int maxTokens, Object outputBuffer);
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

    // Create a buffer for output
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
        let _output_str = unsafe {
            CStr::from_ptr(output.as_mut_ptr() as *const c_char)
                .to_str()
                .unwrap_or("")
        };
        result
    } else {
        result
    }
}

/// Generate text using global model
///
/// Java signature:
/// public static native String generateText(String prompt, int maxTokens);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_generateText(
    mut env: JNIEnv,
    _class: JClass,
    prompt: JString,
    max_tokens: jint,
) -> jstring {
    println!("🔥 GPUFabric JNI: Generating text locally");

    let prompt_str = match env.get_string(&prompt) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let prompt_text = match prompt_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    // Get global model and context pointers
    let model_ptr = GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
    let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);

    if model_ptr.is_null() || context_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Model or context not initialized");
        return match env.new_string("Error: Model not loaded") {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        };
    }

    let prompt_cstr = match CString::new(prompt_text) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    // Create a buffer for output
    let mut output = vec![0u8; 4096];

    let result = gpuf_generate_final_solution_text(
        model_ptr,
        context_ptr,
        prompt_cstr.as_ptr(),
        max_tokens as i32,
        output.as_mut_ptr() as *mut c_char,
        output.len() as i32,
    );

    if result > 0 {
        let output_str = unsafe {
            CStr::from_ptr(output.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("")
        };

        match env.new_string(output_str) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    } else {
        match env.new_string(format!("Error: Generation failed with code {}", result)) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Generate text with sampling parameters
///
/// Java signature:
/// public static native String generateTextWithSampling(String prompt, int maxTokens, float temperature, int topK, float topP, float repeatPenalty);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_generateTextWithSampling(
    mut env: JNIEnv,
    _class: JClass,
    prompt: JString,
    max_tokens: jint,
    temperature: jfloat,
    top_k: jint,
    top_p: jfloat,
    repeat_penalty: jfloat,
) -> jstring {
    println!("🔥 GPUFabric JNI: Generating text with sampling parameters");

    let prompt_str = match env.get_string(&prompt) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let prompt_text = match prompt_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    // Get global model and context pointers
    let model_ptr = GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
    let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);

    if model_ptr.is_null() || context_ptr.is_null() {
        eprintln!("🔥 GPUFabric JNI: Model or context not initialized");
        return match env.new_string("Error: Model not loaded") {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        };
    }

    let prompt_cstr = match CString::new(prompt_text) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    // Create a buffer for output
    let mut output = vec![0u8; 4096];

    let result = manual_llama_completion(
        model_ptr,
        context_ptr,
        prompt_cstr.as_ptr(),
        max_tokens,
        temperature,
        top_k,
        top_p,
        repeat_penalty,
        output.as_mut_ptr(),
        output.len() as c_int,
    );

    if result > 0 {
        let output_str = unsafe {
            CStr::from_ptr(output.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("")
        };

        match env.new_string(output_str) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    } else {
        match env.new_string(format!("Error: Generation failed with code {}", result)) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Check inference service health
///
/// Java signature:
/// public static native String isInferenceServiceHealthy();
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_isInferenceServiceHealthy(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    println!("🔥 GPUFabric JNI: Checking inference service health");

    let status = MODEL_STATUS.lock().unwrap();

    let health_info = if status.is_loaded {
        format!(
            "Healthy - Model: {}, Status: {}",
            status.current_model.as_deref().unwrap_or("None"),
            status.loading_status
        )
    } else {
        format!(
            "Unhealthy - Status: {}, Error: {}",
            status.loading_status,
            status.error_message.as_deref().unwrap_or("None")
        )
    };

    match env.new_string(health_info) {
        Ok(jstring) => jstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// Async Generation with Streaming
// ============================================================================

/// Start async generation with streaming callback
///
/// Java signature:
/// public static native int startGenerationAsync(long ctxPtr, String prompt, int maxTokens, float temperature, int topK, float topP, float repeatPenalty, long callbackFunctionPtr);
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_startGenerationAsync(
    mut env: JNIEnv,
    _class: JClass,
    ctx_ptr: jlong,
    prompt: JString,
    max_tokens: jint,
    temperature: jfloat,
    top_k: jint,
    top_p: jfloat,
    repeat_penalty: jfloat,
    callback_function_ptr: jlong,
) -> jint {
    println!("🚀 JNI: Starting async generation with direct function pointer...");

    let ctx = ctx_ptr as *mut llama_context;
    if ctx.is_null() {
        println!("❌ JNI: Invalid context pointer");
        return -1;
    }

    let prompt_str = match env.get_string(&prompt) {
        Ok(s) => s,
        Err(e) => {
            println!("❌ JNI: Failed to get prompt string: {:?}", e);
            return -1;
        }
    };

    let prompt_cstr = match CString::new(prompt_str.to_string_lossy().to_string()) {
        Ok(s) => s,
        Err(e) => {
            println!("❌ JNI: Failed to create CString: {:?}", e);
            return -1;
        }
    };

    // Convert function pointer
    let callback = if callback_function_ptr != 0 {
        Some(unsafe {
            std::mem::transmute::<jlong, extern "C" fn(*const c_char, *mut c_void)>(
                callback_function_ptr,
            )
        })
    } else {
        None
    };

    let result = gpuf_start_generation_async(
        ctx,
        prompt_cstr.as_ptr(),
        max_tokens,
        temperature,
        top_k,
        top_p,
        repeat_penalty,
        callback,
        std::ptr::null_mut(),
    );

    if result == 0 {
        println!("✅ JNI: Async generation started successfully");
    } else {
        println!("❌ JNI: Failed to start async generation: {}", result);
    }

    result
}

/// Stop ongoing generation
///
/// Java signature:
/// public static native int stopGeneration(long ctxPtr);
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_stopGeneration(
    _env: JNIEnv,
    _class: JClass,
    ctx_ptr: jlong,
) -> jint {
    println!("🛑 JNI: Stopping generation...");

    let ctx = ctx_ptr as *mut llama_context;
    if ctx.is_null() {
        println!("❌ JNI: Invalid context pointer for stop");
        return -1;
    }

    let result = gpuf_stop_generation(ctx);

    if result == 0 {
        println!("✅ JNI: Generation stop signal sent");
    } else {
        println!("❌ JNI: Failed to stop generation: {}", result);
    }

    result
}

/// Check if generation can be started
///
/// Java signature:
/// public static native boolean canStartGeneration(long ctxPtr);
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_canStartGeneration(
    _env: JNIEnv,
    _class: JClass,
    ctx_ptr: jlong,
) -> jboolean {
    let ctx = ctx_ptr as *mut llama_context;
    if ctx.is_null() {
        println!("❌ JNI: Context is null, cannot start generation");
        return 0; // false
    }

    println!("✅ JNI: Context is valid, can start generation");
    1 // true
}

/// Get current generation status
///
/// Java signature:
/// public static native String getGenerationStatus();
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_getGenerationStatus(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let status = if should_stop_generation() {
        "stopping"
    } else {
        "idle"
    };

    match env.new_string(status) {
        Ok(jstring) => jstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// Multimodal API (Vision + Text)
// ============================================================================

/// Load multimodal model (text model + mmproj)
///
/// Java signature:
/// public static native long loadMultimodalModel(String textModelPath, String mmprojPath);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_loadMultimodalModel(
    mut env: JNIEnv,
    _class: JClass,
    text_model_path: JString,
    mmproj_path: JString,
) -> jlong {
    println!("🔥 GPUFabric JNI: Loading multimodal model");

    let text_path_str = match env.get_string(&text_model_path) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let mmproj_path_str = match env.get_string(&mmproj_path) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let text_path = match text_path_str.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let mmproj = match mmproj_path_str.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let text_path_cstr = match CString::new(text_path) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let mmproj_cstr = match CString::new(mmproj) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let multimodal_model =
        gpuf_load_multimodal_model(text_path_cstr.as_ptr(), mmproj_cstr.as_ptr());

    if multimodal_model.is_null() {
        println!("❌ Failed to load multimodal model");
        return 0;
    }

    println!("✅ Multimodal model loaded successfully");
    multimodal_model as jlong
}

/// Create context for multimodal model
///
/// Java signature:
/// public static native long createMultimodalContext(long multimodalModelPtr);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_createMultimodalContext(
    _env: JNIEnv,
    _class: JClass,
    multimodal_model_ptr: jlong,
) -> jlong {
    println!("🔥 GPUFabric JNI: Creating multimodal context");

    if multimodal_model_ptr == 0 {
        println!("❌ Invalid multimodal model pointer");
        return 0;
    }

    let multimodal_model = multimodal_model_ptr as *mut gpuf_multimodal_model;
    let ctx = gpuf_create_multimodal_context(multimodal_model);

    if ctx.is_null() {
        println!("❌ Failed to create multimodal context");
        return 0;
    }

    println!("✅ Multimodal context created successfully");
    ctx as jlong
}

/// Generate with multimodal input (text + image)
///
/// Java signature:
/// public static native String generateMultimodal(long multimodalModelPtr, long ctxPtr, String textPrompt, byte[] imageData, int maxTokens, float temperature, int topK, float topP);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_generateMultimodal(
    mut env: JNIEnv,
    _class: JClass,
    multimodal_model_ptr: jlong,
    ctx_ptr: jlong,
    text_prompt: JString,
    image_data: jbyteArray,
    max_tokens: jint,
    temperature: jfloat,
    top_k: jint,
    top_p: jfloat,
) -> jstring {
    println!("🔥 GPUFabric JNI: Generating with multimodal input");

    if multimodal_model_ptr == 0 || ctx_ptr == 0 {
        println!("❌ Invalid model or context pointer");
        return match env.new_string("Error: Invalid model or context") {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        };
    }

    let prompt_str = match env.get_string(&text_prompt) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let prompt_text = match prompt_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let prompt_cstr = match CString::new(prompt_text) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    // Get image data if provided
    let (image_ptr, image_size) = if !image_data.is_null() {
        // TODO: Implement proper JNI array handling
        (std::ptr::null(), 0)
    } else {
        (std::ptr::null(), 0)
    };

    // Create output buffer
    let mut output = vec![0u8; 4096];

    let result = gpuf_generate_multimodal(
        multimodal_model_ptr as *mut gpuf_multimodal_model,
        ctx_ptr as *mut llama_context,
        prompt_cstr.as_ptr(),
        image_ptr,
        image_size,
        max_tokens,
        temperature,
        top_k,
        top_p,
        1.1, // repeat_penalty
        output.as_mut_ptr() as *mut c_char,
        output.len() as c_int,
    );

    if result > 0 {
        let output_str = unsafe {
            CStr::from_ptr(output.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("")
        };

        match env.new_string(output_str) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    } else {
        match env.new_string(format!("Error: Generation failed with code {}", result)) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Check if multimodal model supports vision
///
/// Java signature:
/// public static native boolean supportsVision(long multimodalModelPtr);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_supportsVision(
    _env: JNIEnv,
    _class: JClass,
    multimodal_model_ptr: jlong,
) -> jboolean {
    if multimodal_model_ptr == 0 {
        return 0;
    }

    let has_vision =
        gpuf_multimodal_supports_vision(multimodal_model_ptr as *mut gpuf_multimodal_model);
    if has_vision {
        1
    } else {
        0
    }
}

/// Free multimodal model
///
/// Java signature:
/// public static native void freeMultimodalModel(long multimodalModelPtr);
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_GPUEngine_freeMultimodalModel(
    _env: JNIEnv,
    _class: JClass,
    multimodal_model_ptr: jlong,
) {
    if multimodal_model_ptr != 0 {
        println!("🔥 GPUFabric JNI: Freeing multimodal model");
        gpuf_free_multimodal_model(multimodal_model_ptr as *mut gpuf_multimodal_model);
        println!("✅ Multimodal model freed");
    }
}
