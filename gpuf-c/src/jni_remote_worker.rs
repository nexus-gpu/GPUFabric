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
use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jbyteArray, jfloat, jint, jlong, jstring};
#[cfg(target_os = "android")]
use jni::{JNIEnv, JavaVM};
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::OnceLock;

use crate::{
    get_remote_worker_status, gpuf_validate_mobile_tls_policy, set_remote_worker_model,
    start_remote_worker, start_remote_worker_tasks_with_callback_ptr, start_remote_worker_with_tls,
    stop_remote_worker,
};

#[cfg(target_os = "android")]
fn status_callback_from_jlong(
    callback_function_ptr: jlong,
) -> Option<extern "C" fn(*const c_char, *mut c_void)> {
    if callback_function_ptr == 0 {
        return None;
    }

    // SAFETY: The Java/Kotlin wrapper passes a native function pointer obtained
    // from trusted native code. The pointer must remain valid for the lifetime
    // of the background worker tasks and must use the expected C ABI/signature.
    Some(unsafe {
        std::mem::transmute::<usize, extern "C" fn(*const c_char, *mut c_void)>(
            callback_function_ptr as usize,
        )
    })
}

#[cfg(target_os = "android")]
fn jstring_to_cstring(
    env: &mut JNIEnv,
    value: &JString,
    label: &str,
) -> Result<std::ffi::CString, ()> {
    let value = env.get_string(value).map_err(|e| {
        eprintln!("❌ JNI: Failed to get {label}: {e}");
    })?;
    let value = value.to_str().map_err(|e| {
        eprintln!("❌ JNI: Failed to convert {label}: {e}");
    })?;
    std::ffi::CString::new(value).map_err(|e| {
        eprintln!("❌ JNI: Failed to create C string for {label}: {e}");
    })
}

#[cfg(target_os = "android")]
static RN_JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

#[cfg(target_os = "android")]
static RN_CALLBACK_EMITTER: OnceLock<Mutex<Option<GlobalRef>>> = OnceLock::new();

#[cfg(target_os = "android")]
fn rn_emit_status(message: &str) {
    let jvm = match RN_JAVA_VM.get() {
        Some(vm) => vm,
        None => {
            eprintln!("❌ JNI: RN JavaVM not initialized (did you call registerCallbackEmitter?)");
            return;
        }
    };

    let emitter = match RN_CALLBACK_EMITTER
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
    {
        Some(e) => e,
        None => {
            eprintln!("❌ JNI: RN callback emitter not registered");
            return;
        }
    };

    let mut env = match jvm.attach_current_thread() {
        Ok(env) => env,
        Err(e) => {
            eprintln!("❌ JNI: Failed to attach current thread: {:?}", e);
            return;
        }
    };

    let jmsg = match env.new_string(message) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ JNI: Failed to create Java string for callback: {:?}", e);
            return;
        }
    };

    let obj = emitter.as_obj();
    if let Err(e) = env.call_method(
        obj,
        "emit",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&jmsg)],
    ) {
        eprintln!("❌ JNI: Failed to call emitter.emit(String): {:?}", e);
    }
}

#[cfg(target_os = "android")]
extern "C" fn rn_status_callback(message: *const c_char, _user_data: *mut c_void) {
    if message.is_null() {
        return;
    }

    // SAFETY: React Native status callbacks are invoked only with a non-null
    // NUL-terminated message pointer that remains valid for the callback call.
    let msg = unsafe { std::ffi::CStr::from_ptr(message) };
    let msg = msg.to_string_lossy();
    rn_emit_status(&msg);
}

// ============================================================================
// JNI Function: Validate Mobile TLS Policy
// ============================================================================
/// Java signature:
/// public static native int validateMobileTlsPolicy(
///     String caCertPath,
///     String serverName,
///     String certSha256Pin
/// );
///
/// Pass an empty string for caCertPath or certSha256Pin when that trust material
/// is not used. At least one of caCertPath or certSha256Pin must be set.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_validateMobileTlsPolicy(
    mut env: JNIEnv,
    _class: JClass,
    ca_cert_path: JString,
    server_name: JString,
    cert_sha256_pin: JString,
) -> jint {
    let ca_cert_path = match env.get_string(&ca_cert_path) {
        Ok(s) => s,
        Err(_) => return -5,
    };
    let server_name = match env.get_string(&server_name) {
        Ok(s) => s,
        Err(_) => return -5,
    };
    let cert_sha256_pin = match env.get_string(&cert_sha256_pin) {
        Ok(s) => s,
        Err(_) => return -5,
    };

    let ca_cert_path = match ca_cert_path.to_str() {
        Ok(s) => s,
        Err(_) => return -5,
    };
    let server_name = match server_name.to_str() {
        Ok(s) => s,
        Err(_) => return -5,
    };
    let cert_sha256_pin = match cert_sha256_pin.to_str() {
        Ok(s) => s,
        Err(_) => return -5,
    };

    let ca_cert_path = match std::ffi::CString::new(ca_cert_path) {
        Ok(s) => s,
        Err(_) => return -5,
    };
    let server_name = match std::ffi::CString::new(server_name) {
        Ok(s) => s,
        Err(_) => return -5,
    };
    let cert_sha256_pin = match std::ffi::CString::new(cert_sha256_pin) {
        Ok(s) => s,
        Err(_) => return -5,
    };

    gpuf_validate_mobile_tls_policy(
        ca_cert_path.as_ptr(),
        server_name.as_ptr(),
        cert_sha256_pin.as_ptr(),
    )
}

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

    println!(
        "📂 JNI: Model path accepted ({} bytes)",
        model_path_rust.len()
    );

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

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_registerCallbackEmitter(
    mut env: JNIEnv,
    _class: JClass,
    emitter: JObject,
) -> jint {
    let vm = match env.get_java_vm() {
        Ok(vm) => vm,
        Err(e) => {
            eprintln!("❌ JNI: Failed to get JavaVM: {:?}", e);
            return -1;
        }
    };
    let _ = RN_JAVA_VM.set(vm);

    let global = match env.new_global_ref(emitter) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ JNI: Failed to create GlobalRef for emitter: {:?}", e);
            return -1;
        }
    };

    let slot = RN_CALLBACK_EMITTER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap();
    *guard = Some(global);

    0
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_startRemoteWorkerTasksWithJavaCallback(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    // Ensure emitter is registered
    let registered = RN_CALLBACK_EMITTER
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.as_ref().map(|_| ())))
        .is_some();

    if !registered {
        eprintln!("❌ JNI: Callback emitter not registered. Call registerCallbackEmitter() first.");
        return -1;
    }

    start_remote_worker_tasks_with_callback_ptr(Some(rn_status_callback))
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
        "📡 JNI: Remote worker config received (control_port={}, proxy_port={}, worker_type={}, server_addr_len={}, client_id_len={})",
        control_port,
        proxy_port,
        worker_type_rust,
        server_addr_rust.len(),
        client_id_rust.len()
    );

    // Convert to C strings
    let server_addr_c = match std::ffi::CString::new(server_addr_rust) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "❌ JNI: Failed to create C string for server address: {}",
                e
            );
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
// JNI Function: Start Remote Worker With TLS
// ============================================================================
/// Starts the remote worker over the TLS-wrapped control protocol.
///
/// Java signature:
/// public static native int startRemoteWorkerWithTls(
///     String serverAddr,
///     int controlPort,
///     int proxyPort,
///     String workerType,
///     String clientId,
///     String caCertPath,
///     String controlTlsServerName,
///     String certSha256Pin
/// );
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_startRemoteWorkerWithTls(
    mut env: JNIEnv,
    _class: JClass,
    server_addr: JString,
    control_port: jint,
    proxy_port: jint,
    worker_type: JString,
    client_id: JString,
    ca_cert_path: JString,
    control_tls_server_name: JString,
    cert_sha256_pin: JString,
) -> jint {
    println!("🔥 GPUFabric JNI: Starting TLS remote worker");

    let server_addr_c = match jstring_to_cstring(&mut env, &server_addr, "server address") {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let worker_type_c = match jstring_to_cstring(&mut env, &worker_type, "worker type") {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let client_id_c = match jstring_to_cstring(&mut env, &client_id, "client ID") {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let ca_cert_path_c = match jstring_to_cstring(&mut env, &ca_cert_path, "CA cert path") {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let control_tls_server_name_c = match jstring_to_cstring(
        &mut env,
        &control_tls_server_name,
        "control TLS server name",
    ) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let cert_sha256_pin_c = match jstring_to_cstring(&mut env, &cert_sha256_pin, "cert SHA256 pin")
    {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let result = start_remote_worker_with_tls(
        server_addr_c.as_ptr(),
        control_port,
        proxy_port,
        worker_type_c.as_ptr(),
        client_id_c.as_ptr(),
        ca_cert_path_c.as_ptr(),
        control_tls_server_name_c.as_ptr(),
        cert_sha256_pin_c.as_ptr(),
    );

    if result == 0 {
        println!("✅ JNI: TLS remote worker started successfully");
    } else {
        eprintln!(
            "❌ JNI: Failed to start TLS remote worker (error: {})",
            result
        );
    }

    result
}

// ============================================================================
// JNI Function: Start Remote Worker Tasks
// ============================================================================
/// Starts the background tasks for the remote worker with optional callback
///
/// Java signature:
/// public static native int startRemoteWorkerTasks(long callbackFunctionPtr);
///
/// @param callbackFunctionPtr Optional function pointer for status updates
/// @return 0 on success, -1 on failure
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_gpuf_c_RemoteWorker_startRemoteWorkerTasks(
    _env: JNIEnv,
    _class: JClass,
    callback_function_ptr: jlong,
) -> jint {
    println!("🔥 GPUFabric JNI: Starting remote worker tasks");

    let callback = status_callback_from_jlong(callback_function_ptr);

    // Call C API with callback
    let result = start_remote_worker_tasks_with_callback_ptr(callback);

    if result == 0 {
        println!("✅ JNI: Remote worker tasks started successfully");
    } else {
        eprintln!(
            "❌ JNI: Failed to start remote worker tasks (error: {})",
            result
        );
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
    let result = get_remote_worker_status(
        buffer.as_mut_ptr() as *mut std::os::raw::c_char,
        buffer.len(),
    );

    if result != 0 {
        eprintln!(
            "❌ JNI: Failed to get remote worker status (error: {})",
            result
        );
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

    println!("📊 JNI: Status received ({} bytes)", status_str.len());

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
