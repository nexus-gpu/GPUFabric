mod handle;
mod llm_engine;
mod util;
mod llama_wrapper;

pub use handle::{WorkerHandle, AutoWorker};

use anyhow::Result;
use tokio_rustls::rustls::crypto::aws_lc_rs;
use tracing::{debug, info};

#[cfg(target_os = "android")]
use android_logger::Config;
#[cfg(target_os = "android")]
use log::LevelFilter;

/// Initialize the library (crypto provider and logging)
pub fn init() -> Result<()> {
    let provider = aws_lc_rs::default_provider();
    provider.install_default().map_err(|e| {
        anyhow::anyhow!(
            "Failed to install default AWS provider: {:?}",
            e
        )
    })?;

    #[cfg(target_os = "android")]
    android_logger::init_once(
        Config::default()
            .with_max_level(LevelFilter::Debug)
            .with_tag("gpuf-c"),
    );

    #[cfg(not(target_os = "android"))]
    util::init_logging();

    Ok(())
}

/// Create a new worker with the given configuration
pub async fn create_worker(args: util::cmd::Args) -> Result<handle::AutoWorker> {
    debug!("Creating worker with args: {:#?}", args);
    info!("Server address: {}:{}", args.server_addr, args.control_port);
    info!("Local service: {}:{}", args.local_addr, args.local_port);
    
    Ok(handle::new_worker(args).await)
}

// Re-export utility types for external use
pub mod config {
    pub use crate::util::cmd::Args;
}

// ============================================================================
// C FFI Layer - Unified C interface for iOS and Android
// ============================================================================

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

// Global error information storage
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn set_last_error(err: String) {
    if let Ok(mut last_error) = LAST_ERROR.lock() {
        *last_error = Some(err);
    }
}

/// Initialize GPUFabric library
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_init() -> i32 {
    match init() {
        Ok(_) => 0,
        Err(e) => {
            set_last_error(format!("Initialization failed: {}", e));
            -1
        }
    }
}

/// Get last error information
/// Returns: Error message string pointer, caller needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_get_last_error() -> *mut c_char {
    if let Ok(last_error) = LAST_ERROR.lock() {
        if let Some(ref err) = *last_error {
            return CString::new(err.as_str())
                .unwrap_or_else(|_| CString::new("Unknown error").unwrap())
                .into_raw();
        }
    }
    std::ptr::null_mut()
}

/// Release string allocated by the library
#[no_mangle]
pub extern "C" fn gpuf_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

/// Create Worker configuration
/// Returns: Configuration handle, returns null on failure
#[no_mangle]
pub extern "C" fn gpuf_create_config(
    server_addr: *const c_char,
    _control_port: u16,
    _local_addr: *const c_char,
    _local_port: u16,
) -> *mut std::ffi::c_void {
    if server_addr.is_null() || _local_addr.is_null() {
        set_last_error("Invalid parameters".to_string());
        return std::ptr::null_mut();
    }

    let _server_addr_str = unsafe {
        match CStr::from_ptr(server_addr).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                set_last_error("Invalid server address".to_string());
                return std::ptr::null_mut();
            }
        }
    };

    let _local_addr_str = unsafe {
        match CStr::from_ptr(_local_addr).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                set_last_error("Invalid local address".to_string());
                return std::ptr::null_mut();
            }
        }
    };

    // Need to create configuration based on actual Args structure
    // Temporarily return null, need to implement complete configuration creation logic
    set_last_error("Not implemented yet".to_string());
    std::ptr::null_mut()
}

/// Release configuration
#[no_mangle]
pub extern "C" fn gpuf_free_config(config: *mut std::ffi::c_void) {
    if !config.is_null() {
        // Implement configuration release logic
    }
}

/// Get version information
#[no_mangle]
pub extern "C" fn gpuf_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// ============================================================================
// LLM Inference Interface
// ============================================================================

/// Initialize LLM engine
/// model_path: Model file path
/// n_ctx: Context size
/// n_gpu_layers: Number of GPU layers (0 means CPU only)
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_llm_init(
    model_path: *const c_char,
    n_ctx: u32,
    n_gpu_layers: u32,
) -> i32 {
    if model_path.is_null() {
        set_last_error("Model path is null".to_string());
        return -1;
    }

    let path_str = unsafe {
        match CStr::from_ptr(model_path).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("Invalid model path".to_string());
                return -1;
            }
        }
    };

    match llama_wrapper::init_global_engine(path_str, n_ctx, n_gpu_layers) {
        Ok(_) => 0,
        Err(e) => {
            set_last_error(format!("Failed to initialize LLM: {}", e));
            -1
        }
    }
}

/// Generate text
/// prompt: Input prompt
/// max_tokens: Maximum number of tokens to generate
/// Returns: Generated text pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_llm_generate(
    prompt: *const c_char,
    max_tokens: usize,
) -> *mut c_char {
    if prompt.is_null() {
        set_last_error("Prompt is null".to_string());
        return std::ptr::null_mut();
    }

    let prompt_str = unsafe {
        match CStr::from_ptr(prompt).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("Invalid prompt".to_string());
                return std::ptr::null_mut();
            }
        }
    };

    match llama_wrapper::generate_text(prompt_str, max_tokens) {
        Ok(text) => {
            CString::new(text)
                .unwrap_or_else(|_| CString::new("").unwrap())
                .into_raw()
        }
        Err(e) => {
            set_last_error(format!("Generation failed: {}", e));
            std::ptr::null_mut()
        }
    }
}
