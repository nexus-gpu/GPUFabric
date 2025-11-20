mod handle;
pub mod util;

// LLM modules are excluded in lightweight Android version

pub mod llm_engine;
#[cfg(not(target_os = "android"))]
mod llama_wrapper;

pub use handle::{WorkerHandle, AutoWorker};

use anyhow::Result;

#[cfg(target_os = "android")]
use android_logger::Config;
#[cfg(target_os = "android")]
use log::LevelFilter;

/// Initialize the library (logging only for lightweight version)
pub fn init() -> Result<()> {
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
    log::debug!("Creating worker with args: {:#?}", args);
    log::info!("Server address: {}:{}", args.server_addr, args.control_port);
    log::info!("Local service: {}:{}", args.local_addr, args.local_port);
    
    Ok(handle::new_worker(args).await)
}

// Re-export utility types for external use
pub mod config {
    pub use crate::util::cmd::Args;
}

// ============================================================================
// C FFI Layer - Lightweight C interface for Android
// ============================================================================

use std::ffi::CString;
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

/// Get version information
#[no_mangle]
pub extern "C" fn gpuf_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// ============================================================================
// LLM Interface - Stubs for lightweight version
// ============================================================================

/// Initialize LLM engine - Not supported in lightweight version
#[no_mangle]
pub extern "C" fn gpuf_llm_init(
    _model_path: *const c_char,
    _n_ctx: u32,
    _n_gpu_layers: u32,
) -> i32 {
    set_last_error("LLM engine not supported in lightweight version".to_string());
    -1
}

/// Generate text - Not supported in lightweight version
#[no_mangle]
pub extern "C" fn gpuf_llm_generate(
    _prompt: *const c_char,
    _max_tokens: usize,
) -> *mut c_char {
    set_last_error("LLM generation not supported in lightweight version".to_string());
    std::ptr::null_mut()
}

/// Check if LLM engine is initialized - Always false in lightweight version
#[no_mangle]
pub extern "C" fn gpuf_llm_is_initialized() -> i32 {
    0
}

/// Unload LLM engine - No-op in lightweight version
#[no_mangle]
pub extern "C" fn gpuf_llm_unload() -> i32 {
    0
}
