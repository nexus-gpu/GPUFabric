mod handle;
pub mod util;
pub mod client_sdk;

pub mod llm_engine;
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
// LLM Interface - Full implementation for SDK
// ============================================================================

use std::ffi::CStr;
use crate::llama_wrapper::{init_global_engine, generate_text, is_initialized, unload_global_engine};
use crate::client_sdk::{GPUFabricClient, ClientConfig};

// Global client instance
static GLOBAL_CLIENT: std::sync::OnceLock<std::sync::Mutex<Option<GPUFabricClient>>> = std::sync::OnceLock::new();

/// Initialize LLM engine with model
/// model_path: Model file path (null-terminated string)
/// n_ctx: Context size for the model
/// n_gpu_layers: Number of GPU layers (0 = CPU only)
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_llm_init(
    model_path: *const c_char,
    n_ctx: u32,
    n_gpu_layers: u32,
) -> i32 {
    if model_path.is_null() {
        set_last_error("Model path cannot be null".to_string());
        return -1;
    }
    
    let path_str = match unsafe { CStr::from_ptr(model_path) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid model path string: {}", e));
            return -1;
        }
    };
    
    match init_global_engine(path_str, n_ctx, n_gpu_layers) {
        Ok(_) => {
            log::info!("LLM engine initialized successfully with model: {}", path_str);
            0
        },
        Err(e) => {
            let error_msg = format!("Failed to initialize LLM engine: {}", e);
            set_last_error(error_msg);
            -1
        }
    }
}

/// Generate text using the initialized LLM engine
/// prompt: Input prompt (null-terminated string)
/// max_tokens: Maximum number of tokens to generate
/// Returns: Generated text pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_llm_generate(
    prompt: *const c_char,
    max_tokens: usize,
) -> *mut c_char {
    if prompt.is_null() {
        set_last_error("Prompt cannot be null".to_string());
        return std::ptr::null_mut();
    }
    
    if !is_initialized() {
        set_last_error("LLM engine not initialized. Call gpuf_llm_init first.".to_string());
        return std::ptr::null_mut();
    }
    
    let prompt_str = match unsafe { CStr::from_ptr(prompt) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid prompt string: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    match generate_text(prompt_str, max_tokens) {
        Ok(response) => {
            log::debug!("Generated {} tokens for prompt: {}", response.len(), prompt_str);
            CString::new(response)
                .unwrap_or_else(|_| CString::new("Generation failed").unwrap())
                .into_raw()
        },
        Err(e) => {
            let error_msg = format!("Text generation failed: {}", e);
            set_last_error(error_msg);
            std::ptr::null_mut()
        }
    }
}

/// Check if LLM engine is initialized
/// Returns: 1 if initialized, 0 if not
#[no_mangle]
pub extern "C" fn gpuf_llm_is_initialized() -> i32 {
    if is_initialized() {
        1
    } else {
        0
    }
}

/// Unload LLM engine and free resources
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_llm_unload() -> i32 {
    match unload_global_engine() {
        Ok(_) => {
            log::info!("LLM engine unloaded successfully");
            0
        },
        Err(e) => {
            let error_msg = format!("Failed to unload LLM engine: {}", e);
            set_last_error(error_msg);
            -1
        }
    }
}

// ============================================================================
// Client SDK Interface - Device monitoring and sharing
// ============================================================================

/// Initialize GPUFabric client with configuration
/// config_json: JSON string with client configuration
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_init(config_json: *const c_char) -> i32 {
    if config_json.is_null() {
        set_last_error("Config JSON cannot be null".to_string());
        return -1;
    }
    
    let config_str = match unsafe { CStr::from_ptr(config_json) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid config JSON string: {}", e));
            return -1;
        }
    };
    
    // Parse configuration
    let config: ClientConfig = match serde_json::from_str(config_str) {
        Ok(c) => c,
        Err(e) => {
            set_last_error(format!("Failed to parse config JSON: {}", e));
            return -1;
        }
    };
    
    // Create client instance
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            set_last_error(format!("Failed to create runtime: {}", e));
            return -1;
        }
    };
    
    let client = rt.block_on(GPUFabricClient::new(config));
    
    // Store to global variable
    let global = GLOBAL_CLIENT.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut guard) = global.lock() {
        *guard = Some(client);
        log::info!("GPUFabric client initialized successfully");
        0
    } else {
        set_last_error("Failed to acquire client lock".to_string());
        -1
    }
}

/// Connect and register the client to the server
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_connect() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized. Call gpuf_client_init first.".to_string());
            return -1;
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return -1;
        }
    };
    
    if let Some(client) = guard.as_ref() {
        // Use tokio runtime to execute async operations
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return -1;
            }
        };
        
        match rt.block_on(client.connect_and_register()) {
            Ok(_) => {
                log::info!("Client connected and registered successfully");
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to connect client: {}", e));
                -1
            }
        }
    } else {
        set_last_error("Client not initialized".to_string());
        -1
    }
}

/// Get client status as JSON string
/// Returns: Status JSON string pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_client_get_status() -> *mut c_char {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return std::ptr::null_mut();
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return std::ptr::null_mut();
            }
        };
        
        let status = rt.block_on(client.get_status());
        let status_json = serde_json::to_string(&status).unwrap_or_default();
        
        CString::new(status_json)
            .unwrap_or_else(|_| CString::new("Status serialization failed").unwrap())
            .into_raw()
    } else {
        set_last_error("Client not initialized".to_string());
        std::ptr::null_mut()
    }
}

/// Get device information as JSON string
/// Returns: Device info JSON string pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_client_get_device_info() -> *mut c_char {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return std::ptr::null_mut();
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return std::ptr::null_mut();
            }
        };
        
        let device_info = rt.block_on(client.get_device_info());
        let info_json = serde_json::to_string(&device_info).unwrap_or_default();
        
        CString::new(info_json)
            .unwrap_or_else(|_| CString::new("Device info serialization failed").unwrap())
            .into_raw()
    } else {
        set_last_error("Client not initialized".to_string());
        std::ptr::null_mut()
    }
}

/// Get client metrics as JSON string
/// Returns: Metrics JSON string pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_client_get_metrics() -> *mut c_char {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return std::ptr::null_mut();
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return std::ptr::null_mut();
            }
        };
        
        let metrics = rt.block_on(client.get_metrics());
        let metrics_json = serde_json::to_string(&metrics).unwrap_or_default();
        
        CString::new(metrics_json)
            .unwrap_or_else(|_| CString::new("Metrics serialization failed").unwrap())
            .into_raw()
    } else {
        set_last_error("Client not initialized".to_string());
        std::ptr::null_mut()
    }
}

/// Update device information
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_update_device_info() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return -1;
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return -1;
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return -1;
            }
        };
        
        match rt.block_on(client.update_device_info()) {
            Ok(_) => {
                log::info!("Device information updated successfully");
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to update device info: {}", e));
                -1
            }
        }
    } else {
        set_last_error("Client not initialized".to_string());
        -1
    }
}

/// Disconnect client from server
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_disconnect() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return -1;
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return -1;
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return -1;
            }
        };
        
        match rt.block_on(client.disconnect()) {
            Ok(_) => {
                log::info!("Client disconnected successfully");
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to disconnect client: {}", e));
                -1
            }
        }
    } else {
        set_last_error("Client not initialized".to_string());
        -1
    }
}

/// Cleanup client resources
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_cleanup() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return -1;
        }
    };
    
    if let Ok(mut guard) = global.lock() {
        if let Some(client) = guard.take() {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    set_last_error(format!("Failed to create runtime: {}", e));
                    return -1;
                }
            };
            
            match rt.block_on(client.disconnect()) {
                Ok(_) => {
                    log::info!("Client cleaned up successfully");
                    0
                },
                Err(e) => {
                    set_last_error(format!("Failed to cleanup client: {}", e));
                    -1
                }
            }
        } else {
            log::info!("Client already cleaned up");
            0
        }
    } else {
        set_last_error("Failed to acquire client lock".to_string());
        -1
    }
}
