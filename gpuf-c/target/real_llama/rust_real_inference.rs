use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

// LLAMA.cpp C 绑定
#[repr(C)]
pub struct llama_model {
    _private: [u8; 0],
}

#[repr(C)]
pub struct llama_context {
    _private: [u8; 0],
}

#[link(name = "llama_android", kind = "static")]
extern "C" {
    fn llama_backend_init();
    fn llama_backend_free();
    fn llama_load_model_from_file(path: *const c_char, params: c_int) -> *mut llama_model;
    fn llama_free_model(model: *mut llama_model);
    fn llama_new_context_with_model(model: *mut llama_model, params: c_int) -> *mut llama_context;
    fn llama_free(ctx: *mut llama_context);
    fn llama_generate_text(ctx: *mut llama_context, prompt: *const c_char, max_tokens: c_int) -> *mut c_char;
}

use std::sync::Mutex;

static mut REAL_INFERENCE_STATE: Option<Mutex<RealInferenceState>> = None;

struct RealInferenceState {
    initialized: bool,
    backend_initialized: bool,
    loaded_model: Option<*mut llama_model>,
    context: Option<*mut llama_context>,
    model_path: Option<String>,
    last_error: Option<CString>,
}

impl RealInferenceState {
    fn new() -> Self {
        Self {
            initialized: false,
            backend_initialized: false,
            loaded_model: None,
            context: None,
            model_path: None,
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
    
    fn init_backend(&mut self) -> bool {
        if !self.backend_initialized {
            unsafe {
                llama_backend_init();
                self.backend_initialized = true;
            }
        }
        true
    }
    
    fn load_real_model(&mut self, path: &str) -> Result<(), String> {
        if !self.backend_initialized {
            return Err("Backend not initialized".to_string());
        }
        
        unsafe {
            let c_path = CString::new(path).map_err(|e| e.to_string())?;
            let model = llama_load_model_from_file(c_path.as_ptr(), 0);
            
            if model.is_null() {
                return Err("Failed to load model".to_string());
            }
            
            let context = llama_new_context_with_model(model, 0);
            if context.is_null() {
                llama_free_model(model);
                return Err("Failed to create context".to_string());
            }
            
            // 清理之前的模型
            if let Some(old_ctx) = self.context {
                llama_free(old_ctx);
            }
            if let Some(old_model) = self.loaded_model {
                llama_free_model(old_model);
            }
            
            self.loaded_model = Some(model);
            self.context = Some(context);
            self.model_path = Some(path.to_string());
            
            Ok(())
        }
    }
    
    fn generate_real_text(&mut self, prompt: &str, max_tokens: c_int) -> Result<String, String> {
        if let Some(context) = self.context {
            unsafe {
                let c_prompt = CString::new(prompt).map_err(|e| e.to_string())?;
                let result = llama_generate_text(context, c_prompt.as_ptr(), max_tokens);
                
                if result.is_null() {
                    return Err("Generation failed".to_string());
                }
                
                let c_str = CStr::from_ptr(result);
                let response = c_str.to_str().map_err(|e| e.to_string())?;
                
                Ok(response.to_string())
            }
        } else {
            Err("No model loaded".to_string())
        }
    }
}

fn get_real_state() -> &'static Mutex<RealInferenceState> {
    unsafe {
        if REAL_INFERENCE_STATE.is_none() {
            REAL_INFERENCE_STATE = Some(Mutex::new(RealInferenceState::new()));
        }
        REAL_INFERENCE_STATE.as_ref().unwrap()
    }
}

// 真实推理 API
#[no_mangle]
pub extern "C" fn gpuf_real_init() -> c_int {
    let state = get_real_state();
    let mut guard = state.lock().unwrap();
    
    if guard.initialized {
        return 1;
    }
    
    if !guard.init_backend() {
        guard.set_error("Failed to initialize LLAMA.cpp backend");
        return -1;
    }
    
    guard.initialized = true;
    guard.set_error("No error");
    
    0
}

#[no_mangle]
pub extern "C" fn gpuf_real_load_model(model_path: *const c_char) -> c_int {
    if model_path.is_null() {
        let state = get_real_state();
        let mut guard = state.lock().unwrap();
        guard.set_error("Model path is null");
        return -1;
    }
    
    let state = get_real_state();
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
        
        if let Err(e) = guard.load_real_model(path_str) {
            guard.set_error(&e);
            return -1;
        }
        
        0
    }
}

#[no_mangle]
pub extern "C" fn gpuf_real_generate(prompt: *const c_char, max_tokens: c_int) -> *const c_char {
    if prompt.is_null() {
        let state = get_real_state();
        let mut guard = state.lock().unwrap();
        guard.set_error("Prompt is null");
        return std::ptr::null();
    }
    
    let state = get_real_state();
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
        
        match guard.generate_real_text(prompt_str, max_tokens) {
            Ok(response) => {
                match CString::new(response) {
                    Ok(c_string) => c_string.into_raw(),
                    Err(_) => {
                        guard.set_error("Failed to create response string");
                        std::ptr::null()
                    }
                }
            }
            Err(e) => {
                guard.set_error(&e);
                std::ptr::null()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn gpuf_real_cleanup() -> c_int {
    let state = get_real_state();
    let mut guard = state.lock().unwrap();
    
    unsafe {
        if let Some(context) = guard.context {
            llama_free(context);
        }
        if let Some(model) = guard.loaded_model {
            llama_free_model(model);
        }
        
        if guard.backend_initialized {
            llama_backend_free();
            guard.backend_initialized = false;
        }
    }
    
    guard.initialized = false;
    guard.last_error = None;
    
    0
}

#[no_mangle]
pub extern "C" fn gpuf_real_get_last_error() -> *const c_char {
    let state = get_real_state();
    let guard = state.lock().unwrap();
    guard.get_error()
}

#[no_mangle]
pub extern "C" fn gpuf_real_version() -> *const c_char {
    let version = CString::new("1.0.0-real-llama-inference").unwrap();
    version.into_raw()
}

#[no_mangle]
pub extern "C" fn gpuf_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}
