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
// C FFI Layer - 统一的 C 接口，供 iOS 和 Android 使用
// ============================================================================

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

// 全局错误信息存储
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn set_last_error(err: String) {
    if let Ok(mut last_error) = LAST_ERROR.lock() {
        *last_error = Some(err);
    }
}

/// 初始化 GPUFabric 库
/// 返回: 0 成功, -1 失败
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

/// 获取最后一次错误信息
/// 返回: 错误信息字符串指针，调用者需要调用 gpuf_free_string 释放
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

/// 释放由库分配的字符串
#[no_mangle]
pub extern "C" fn gpuf_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

/// 创建 Worker 配置
/// 返回: 配置句柄，失败返回 null
#[no_mangle]
pub extern "C" fn gpuf_create_config(
    server_addr: *const c_char,
    control_port: u16,
    local_addr: *const c_char,
    local_port: u16,
) -> *mut std::ffi::c_void {
    if server_addr.is_null() || local_addr.is_null() {
        set_last_error("Invalid parameters".to_string());
        return std::ptr::null_mut();
    }

    let server_addr_str = unsafe {
        match CStr::from_ptr(server_addr).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                set_last_error("Invalid server address".to_string());
                return std::ptr::null_mut();
            }
        }
    };

    let local_addr_str = unsafe {
        match CStr::from_ptr(local_addr).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                set_last_error("Invalid local address".to_string());
                return std::ptr::null_mut();
            }
        }
    };

    // 这里需要根据实际的 Args 结构创建配置
    // 暂时返回 null，需要实现完整的配置创建逻辑
    set_last_error("Not implemented yet".to_string());
    std::ptr::null_mut()
}

/// 释放配置
#[no_mangle]
pub extern "C" fn gpuf_free_config(config: *mut std::ffi::c_void) {
    if !config.is_null() {
        // 实现配置释放逻辑
    }
}

/// 获取版本信息
#[no_mangle]
pub extern "C" fn gpuf_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// ============================================================================
// LLM 推理接口
// ============================================================================

/// 初始化 LLM 引擎
/// model_path: 模型文件路径
/// n_ctx: 上下文大小
/// n_gpu_layers: GPU 层数（0 表示 CPU only）
/// 返回: 0 成功, -1 失败
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

/// 生成文本
/// prompt: 输入提示词
/// max_tokens: 最大生成 token 数
/// 返回: 生成的文本指针，需要调用 gpuf_free_string 释放
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
