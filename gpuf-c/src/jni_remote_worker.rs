// ============================================================================
// JNI Wrappers for Remote Worker C API
// ============================================================================
//
// This module provides JNI bindings for the Remote Worker C API functions
// used in android_test.c, allowing them to be called from Java/Kotlin code.
//
// Package: com.gpuf.c.RemoteWorker
// ============================================================================

#[cfg(target_os = "android")]
use jni::objects::{JClass, JString};
#[cfg(target_os = "android")]
use jni::sys::{jint, jstring};
#[cfg(target_os = "android")]
use jni::JNIEnv;

use crate::{
    get_remote_worker_status, set_remote_worker_model, start_remote_worker,
    start_remote_worker_tasks, stop_remote_worker,
};

// ============================================================================
// JNI Function: Set Remote Worker Model
// ============================================================================
/// Sets the model path for the remote worker (hot swapping support)
///
/// Java signature:
/// public static native int setRemoteWorkerModel(String modelPath);
///
/// @param modelPath Path to the GGUF model file
/// @return 0 on success, -1 on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_setRemoteWorkerModel(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) -> jint {
    println!("🔥 GPUFabric JNI: Setting remote worker model");

    // Convert JString to Rust string
    let model_path_str = match env.get_string(&model_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to get model path string: {}", e);
            return -1;
        }
    };

    let model_path_rust = match model_path_str.to_str() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to convert model path to UTF-8: {}", e);
            return -1;
        }
    };

    println!("📂 JNI: Model path: {}", model_path_rust);

    // Convert to C string
    let model_path_c = match std::ffi::CString::new(model_path_rust) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to create C string: {}", e);
            return -1;
        }
    };

    // Call C API
    let result = set_remote_worker_model(model_path_c.as_ptr());

    if result == 0 {
        println!("✅ JNI: Model set successfully");
    } else {
        eprintln!("❌ JNI: Failed to set model (error: {})", result);
    }

    result
}

// ============================================================================
// JNI Function: Start Remote Worker
// ============================================================================
/// Starts the remote worker connection to the server
///
/// Java signature:
/// public static native int startRemoteWorker(
///     String serverAddr,
///     int controlPort,
///     int proxyPort,
///     String workerType,
///     String clientId
/// );
///
/// @param serverAddr Server IP address or hostname
/// @param controlPort Control port number
/// @param proxyPort Proxy port number
/// @param workerType Worker type ("TCP" or "WS")
/// @param clientId Client ID (32 hex characters)
/// @return 0 on success, -1 on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_startRemoteWorker(
    mut env: JNIEnv,
    _class: JClass,
    server_addr: JString,
    control_port: jint,
    proxy_port: jint,
    worker_type: JString,
    client_id: JString,
) -> jint {
    println!("🔥 GPUFabric JNI: Starting remote worker");

    // Convert server address
    let server_addr_str = match env.get_string(&server_addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to get server address: {}", e);
            return -1;
        }
    };
    let server_addr_rust = match server_addr_str.to_str() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to convert server address: {}", e);
            return -1;
        }
    };

    // Convert worker type
    let worker_type_str = match env.get_string(&worker_type) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to get worker type: {}", e);
            return -1;
        }
    };
    let worker_type_rust = match worker_type_str.to_str() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to convert worker type: {}", e);
            return -1;
        }
    };

    // Convert client ID
    let client_id_str = match env.get_string(&client_id) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to get client ID: {}", e);
            return -1;
        }
    };
    let client_id_rust = match client_id_str.to_str() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to convert client ID: {}", e);
            return -1;
        }
    };

    println!(
        "📡 JNI: Connecting to {}:{}/{} as {} (type: {})",
        server_addr_rust, control_port, proxy_port, client_id_rust, worker_type_rust
    );

    // Convert to C strings
    let server_addr_c = match std::ffi::CString::new(server_addr_rust) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to create C string for server address: {}", e);
            return -1;
        }
    };

    let worker_type_c = match std::ffi::CString::new(worker_type_rust) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to create C string for worker type: {}", e);
            return -1;
        }
    };

    let client_id_c = match std::ffi::CString::new(client_id_rust) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to create C string for client ID: {}", e);
            return -1;
        }
    };

    // Call C API
    let result = start_remote_worker(
        server_addr_c.as_ptr(),
        control_port,
        proxy_port,
        worker_type_c.as_ptr(),
        client_id_c.as_ptr(),
    );

    if result == 0 {
        println!("✅ JNI: Remote worker started successfully");
    } else {
        eprintln!("❌ JNI: Failed to start remote worker (error: {})", result);
    }

    result
}

// ============================================================================
// JNI Function: Start Remote Worker Tasks
// ============================================================================
/// Starts the background tasks for the remote worker
///
/// Java signature:
/// public static native int startRemoteWorkerTasks();
///
/// @return 0 on success, -1 on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_startRemoteWorkerTasks(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    println!("🔥 GPUFabric JNI: Starting remote worker tasks");

    // Call C API
    let result = start_remote_worker_tasks();

    if result == 0 {
        println!("✅ JNI: Remote worker tasks started successfully");
    } else {
        eprintln!("❌ JNI: Failed to start remote worker tasks (error: {})", result);
    }

    result
}

// ============================================================================
// JNI Function: Get Remote Worker Status
// ============================================================================
/// Gets the current status of the remote worker
///
/// Java signature:
/// public static native String getRemoteWorkerStatus();
///
/// @return Status string or null on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_getRemoteWorkerStatus(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    println!("🔥 GPUFabric JNI: Getting remote worker status");

    // Allocate buffer for status
    let mut buffer = vec![0u8; 1024];

    // Call C API
    let result = get_remote_worker_status(buffer.as_mut_ptr() as *mut std::os::raw::c_char, buffer.len());

    if result != 0 {
        eprintln!("❌ JNI: Failed to get remote worker status (error: {})", result);
        return std::ptr::null_mut();
    }

    // Find null terminator
    let null_pos = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let status_bytes = &buffer[..null_pos];

    // Convert to Rust string
    let status_str = match std::str::from_utf8(status_bytes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to convert status to UTF-8: {}", e);
            return std::ptr::null_mut();
        }
    };

    println!("📊 JNI: Status: {}", status_str);

    // Convert to JString
    match env.new_string(status_str) {
        Ok(jstr) => jstr.into_raw(),
        Err(e) => {
            eprintln!("❌ JNI: Failed to create JString: {}", e);
            std::ptr::null_mut()
        }
    }
}

// ============================================================================
// JNI Function: Stop Remote Worker
// ============================================================================
/// Stops the remote worker and cleans up resources
///
/// Java signature:
/// public static native int stopRemoteWorker();
///
/// @return 0 on success, -1 on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_stopRemoteWorker(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    println!("🔥 GPUFabric JNI: Stopping remote worker");

    // Call C API
    let result = stop_remote_worker();

    if result == 0 {
        println!("✅ JNI: Remote worker stopped successfully");
    } else {
        eprintln!("❌ JNI: Failed to stop remote worker (error: {})", result);
    }

    result
}
