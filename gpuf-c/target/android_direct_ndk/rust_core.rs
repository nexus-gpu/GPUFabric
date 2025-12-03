use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::Mutex;
use std::collections::HashMap;

// 全局状态管理
static mut GLOBAL_STATE: Option<Mutex<GlobalState>> = None;

struct GlobalState {
    initialized: bool,
    models: HashMap<String, ModelHandle>,
    last_error: Option<CString>,
}

struct ModelHandle {
    id: String,
    loaded: bool,
}

impl GlobalState {
    fn new() -> Self {
        Self {
            initialized: false,
            models: HashMap::new(),
            last_error: None,
        }
    }
    
    fn set_error(&mut self, error: &str) {
        self.last_error = Some(CString::new(error).unwrap());
    }
    
    fn get_error(&self) -> *const c_char {
        match self.last_error.as_ref() {
            Some(error) => error.as_ptr(),
            None => std::ptr::null(),
        }
    }
}

// 获取全局状态
fn get_global_state() -> &'static Mutex<GlobalState> {
    unsafe {
        if GLOBAL_STATE.is_none() {
            GLOBAL_STATE = Some(Mutex::new(GlobalState::new()));
        }
        GLOBAL_STATE.as_ref().unwrap()
    }
}

// 核心初始化函数
#[no_mangle]
pub extern "C" fn gpuf_init() -> c_int {
    let state = get_global_state();
    let mut guard = state.lock().unwrap();
    
    if guard.initialized {
        return 1; // Already initialized
    }
    
    // 这里可以初始化 LLAMA.cpp
    // 暂时模拟成功
    guard.initialized = true;
    guard.set_error("No error");
    
    0 // Success
}

// 清理函数
#[no_mangle]
pub extern "C" fn gpuf_cleanup() -> c_int {
    let state = get_global_state();
    let mut guard = state.lock().unwrap();
    
    if !guard.initialized {
        return 1; // Not initialized
    }
    
    // 清理所有模型
    guard.models.clear();
    guard.initialized = false;
    guard.last_error = None;
    
    0 // Success
}

// 版本信息
#[no_mangle]
pub extern "C" fn gpuf_version() -> *const c_char {
    let version = CString::new("1.0.0-android-direct-ndk").unwrap();
    version.into_raw()
}

// 获取最后一个错误
#[no_mangle]
pub extern "C" fn gpuf_get_last_error() -> *const c_char {
    let state = get_global_state();
    let guard = state.lock().unwrap();
    guard.get_error()
}

// 加载模型
#[no_mangle]
pub extern "C" fn gpuf_llm_load_model(model_path: *const c_char) -> c_int {
    if model_path.is_null() {
        let state = get_global_state();
        let mut guard = state.lock().unwrap();
        guard.set_error("Model path is null");
        return -1;
    }
    
    let state = get_global_state();
    let mut guard = state.lock().unwrap();
    
    unsafe {
        let c_str = CStr::from_ptr(model_path);
        let path_str = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => {
                guard.set_error("Invalid model path encoding");
                return -1;
            }
        };
        
        // 这里应该调用 LLAMA.cpp 加载模型
        // 暂时模拟成功
        let model_handle = ModelHandle {
            id: path_str.to_string(),
            loaded: true,
        };
        
        guard.models.insert(path_str.to_string(), model_handle);
        
        0 // Success
    }
}

// 生成文本
#[no_mangle]
pub extern "C" fn gpuf_llm_generate(prompt: *const c_char, max_tokens: c_int) -> *const c_char {
    if prompt.is_null() {
        let state = get_global_state();
        let mut guard = state.lock().unwrap();
        guard.set_error("Prompt is null");
        return std::ptr::null();
    }
    
    let state = get_global_state();
    let mut guard = state.lock().unwrap();
    
    unsafe {
        let c_str = CStr::from_ptr(prompt);
        let prompt_str = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => {
                guard.set_error("Invalid prompt encoding");
                return std::ptr::null();
            }
        };
        
        // 这里应该调用 LLAMA.cpp 生成
        let response = format!("🤖 GPUFabric Response: {} (max_tokens: {})", prompt_str, max_tokens);
        
        match CString::new(response) {
            Ok(c_string) => c_string.into_raw(),
            Err(_) => {
                guard.set_error("Failed to create response string");
                std::ptr::null()
            }
        }
    }
}

// 卸载模型
#[no_mangle]
pub extern "C" fn gpuf_llm_unload() -> c_int {
    let state = get_global_state();
    let mut guard = state.lock().unwrap();
    
    guard.models.clear();
    
    0 // Success
}

// 内存管理
#[no_mangle]
pub extern "C" fn gpuf_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

// 扩展功能: 获取模型数量
#[no_mangle]
pub extern "C" fn gpuf_get_model_count() -> c_int {
    let state = get_global_state();
    let guard = state.lock().unwrap();
    guard.models.len() as c_int
}

// 扩展功能: 检查模型是否已加载
#[no_mangle]
pub extern "C" fn gpuf_is_model_loaded(model_path: *const c_char) -> c_int {
    if model_path.is_null() {
        return 0;
    }
    
    let state = get_global_state();
    let guard = state.lock().unwrap();
    
    unsafe {
        let c_str = CStr::from_ptr(model_path);
        let path_str = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        
        if guard.models.contains_key(path_str) {
            1
        } else {
            0
        }
    }
}

// 性能统计
#[no_mangle]
pub extern "C" fn gpuf_get_performance_stats() -> *const c_char {
    let state = get_global_state();
    let guard = state.lock().unwrap();
    
    let stats = format!(
        "GPUFabric Performance Stats:\n- Initialized: {}\n- Models Loaded: {}\n- Version: {}",
        guard.initialized,
        guard.models.len(),
        "1.0.0-android-direct-ndk"
    );
    
    match CString::new(stats) {
        Ok(c_string) => c_string.into_raw(),
        Err(_) => std::ptr::null(),
    }
}
