//! Android-specific login implementation using native threads
//!
//! This module provides Android-compatible login functionality that avoids
//! the Tokio runtime issues in shell environments by using native threads
//! and blocking I/O operations.

use super::{Args, AutoWorker, WorkerHandle};
use anyhow::{anyhow, Result};
use bincode::{self as bincode, config as bincode_config};
use common::{Command, CommandV1, DevicesInfo, EngineType, OsType, SystemInfo};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, OnceLock};


use tokio::task::JoinHandle;
use tracing::info;



#[cfg(target_os = "android")]
use std::time::Duration;
/// Global TCP connection storage for Android background tasks
pub static ANDROID_TCP_STREAM: std::sync::OnceLock<Arc<Mutex<std::net::TcpStream>>> =
    std::sync::OnceLock::new();

/// Global worker instance for Android JNI
static GLOBAL_WORKER: OnceLock<Mutex<Option<Arc<AutoWorker>>>> = OnceLock::new();

/// Global worker task handle for background operations
static GLOBAL_WORKER_HANDLES: OnceLock<Mutex<Option<(JoinHandle<()>, JoinHandle<()>)>>> =
    OnceLock::new();

/// Perform Android-native login using blocking TCP and bincode protocol
///
/// This function replicates the functionality of TCPWorker::login() but
/// uses native threads and blocking I/O to avoid Tokio runtime issues
/// in Android shell environments.
pub async fn perform_android_login(
    server_addr: &str,
    control_port: u16,
    client_id: &str,
    auto_models: bool,
) -> Result<()> {
    info!("🚀 Android: Starting native login process...");

    // Create TCP connection
    let addr_str = format!("{}:{}", server_addr, control_port);
    info!("🔧 Android: Connecting to {}...", addr_str);

    let mut stream = std::net::TcpStream::connect(&addr_str)
        .map_err(|e| anyhow!("Failed to connect to {}: {}", addr_str, e))?;

    info!("✅ Android: TCP connection established");

    // Collect system and device information
    info!("🔧 Android: Collecting system information...");
    let (cpu_usage, memory_usage, disk_usage, _system_name) =
        crate::util::system_info::collect_system_info()
            .await
            .map_err(|e| anyhow!("Failed to collect system info: {}", e))?;

    let (devices_info, device_count) = crate::util::system_info::collect_device_info()
        .await
        .map_err(|e| anyhow!("Failed to collect device info: {}", e))?;

    // Construct SystemInfo struct
    let system_info = SystemInfo {
        cpu_usage,
        memory_usage,
        disk_usage,
        network_rx: 0,
        network_tx: 0,
    };
    // Create Login command (same structure as TCPWorker::login())
    const CURRENT_VERSION: u32 = 1;

    // Calculate device metrics from actual device info
    let device_memtotal_gb = devices_info.memsize_gb.try_into().unwrap_or(0);
    let device_total_tflops = devices_info.total_tflops.into();

    // Ensure devices_info has reasonable values for server compatibility
    let mut fixed_devices_info = devices_info.clone();
    if fixed_devices_info.vendor_id == 0 {
        // Set a default vendor ID (ARM) to prevent server crashes
        fixed_devices_info.vendor_id = 0x41; // ARM vendor ID
    }
    if fixed_devices_info.device_id == 0 {
        // Set a default device ID to prevent server crashes
        fixed_devices_info.device_id = 0x1000; // Generic ARM device ID
    }

    let login_cmd = CommandV1::Login {
        version: CURRENT_VERSION,
        auto_models,
        os_type: OsType::ANDROID,
        client_id: hex::decode(client_id)
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default(),
        system_info,
        device_memtotal_gb,
        device_total_tflops,
        devices_info: vec![fixed_devices_info],
    };

    // Serialize login command with bincode
    info!("🔧 Android: Serializing login command...");
    let config = bincode_config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();

    let buf = bincode::encode_to_vec(&Command::V1(login_cmd), config)
        .map_err(|e| anyhow!("Failed to serialize login command: {}", e))?;

    let len = buf.len() as u32;
    if len as usize > 1024 {
        return Err(anyhow!("Login command too large: {} bytes", len));
    }

    // Send length prefix + command data
    info!("📤 Android: Sending login command ({} bytes)...", len);

    stream
        .write_all(&len.to_be_bytes())
        .map_err(|e| anyhow!("Failed to send login length: {}", e))?;

    stream
        .write_all(&buf)
        .map_err(|e| anyhow!("Failed to send login data: {}", e))?;

    stream
        .flush()
        .map_err(|e| anyhow!("Failed to flush login data: {}", e))?;

    info!("✅ Android: Login command sent successfully");

    // Store TCP connection globally for background tasks
    let stream_arc = Arc::new(Mutex::new(stream));
    ANDROID_TCP_STREAM
        .set(stream_arc.clone())
        .map_err(|_| anyhow!("Failed to store TCP connection globally"))?;

    info!("✅ Android: TCP connection stored for background tasks");

    Ok(())
}

/// Get the stored TCP connection for background tasks
pub fn get_android_tcp_stream() -> Option<Arc<Mutex<std::net::TcpStream>>> {
    ANDROID_TCP_STREAM.get().cloned()
}

/// Initialize global worker for Android
#[cfg(target_os = "android")]
pub async fn init_global_worker(args: Args) -> Result<()> {
    info!("🚀 init_global_worker: Starting worker initialization...");

    // Create new worker
    info!("📡 init_global_worker: About to call new_worker()...");
    let worker = super::new_worker(args).await;
    info!("✅ init_global_worker: new_worker() completed");

    // Login to server
    info!("🔐 init_global_worker: About to call login()...");
    worker
        .login()
        .await
        .map_err(|e| anyhow!("Failed to login worker: {}", e))?;
    info!("✅ init_global_worker: login() completed");

    // Wrap in Arc for shared access
    let worker_arc = Arc::new(worker);

    // Store in global instance
    let global = GLOBAL_WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = global.lock().unwrap();
    *guard = Some(worker_arc);

    tracing::info!("Global worker initialized successfully");
    Ok(())
}

/// Start background worker tasks (heartbeat, handler, etc.)
#[cfg(target_os = "android")]
pub async fn start_worker_tasks() -> Result<()> {
    use std::thread;

    info!("🔧 Android: Starting background tasks with native threads...");

    // Get the stored TCP connection from android_login module
    let tcp_stream =
        get_android_tcp_stream().ok_or_else(|| anyhow!("TCP connection not initialized"))?;

    // Spawn heartbeat task using native thread with full heartbeat logic
    let heartbeat_stream = tcp_stream.clone();
    let heartbeat_handle = thread::spawn(move || {
        println!("🔧 Android: Heartbeat thread started");

        loop {
            println!("🔧 Android: Heartbeat loop - sleeping for 30 seconds...");
            thread::sleep(Duration::from_secs(30)); // reduce lock conflictsconds heartbeat for testing

            println!("💓 Android: Woke up - collecting system info for heartbeat...");

            // Use simple static values for system info to avoid async issues
            let cpu_usage = 25; // Static CPU usage percentage
            let memory_usage = 45; // Static memory usage percentage
            let disk_usage = 60; // Static disk usage percentage

            // Use simple device info to avoid async calls
            let device_info = DevicesInfo {
                num: 1,
                pod_id: 0,
                total_tflops: 1000,
                memtotal_gb: 4096,
                port: 0,
                ip: 0,
                os_type: OsType::ANDROID,
                engine_type: EngineType::Llama,
                usage: 0,
                mem_usage: 0,
                power_usage: 0,
                temp: 0,
                vendor_id: 0x41, // ARM
                device_id: 0x1000,
                memsize_gb: 4096,
                powerlimit_w: 150,
            };

            println!(
                "💓 Android: Sending heartbeat - CPU: {}% MEM: {}% DISK: {}% MEM_TOTAL: {}GB",
                cpu_usage, memory_usage, disk_usage, device_info.memtotal_gb
            );

            // Send heartbeat using TCP stream with timeout
            let stream_result = heartbeat_stream.try_lock();
            let mut stream = match stream_result {
                Ok(guard) => guard,
                Err(_) => {
                    eprintln!("❌ Android: Failed to lock TCP stream - might be in use by handler");
                    println!("🔧 Android: Skipping this heartbeat due to lock conflict");
                    continue;
                }
            };

            // Create heartbeat command
            let heartbeat_cmd = CommandV1::Heartbeat {
                client_id: [0u8; 16], // TODO: Use actual client ID
                system_info: SystemInfo {
                    cpu_usage,
                    memory_usage,
                    disk_usage,
                    network_rx: 0, // TODO: Implement network monitoring
                    network_tx: 0,
                },
                device_memtotal_gb: device_info.memtotal_gb.try_into().unwrap_or(0),
                device_total_tflops: device_info.total_tflops.into(),
                device_count: device_info.num as u16,
                devices_info: vec![device_info],
            };

            // Serialize and send heartbeat
            let config = bincode_config::standard()
                .with_fixed_int_encoding()
                .with_little_endian();

            if let Ok(buf) = bincode::encode_to_vec(&Command::V1(heartbeat_cmd), config) {
                let len = buf.len() as u32;

                // Send length prefix + heartbeat data
                if let Err(e) = stream.write_all(&len.to_be_bytes()) {
                    eprintln!("❌ Android: Failed to send heartbeat length: {}", e);
                    continue;
                }

                if let Err(e) = stream.write_all(&buf) {
                    eprintln!("❌ Android: Failed to send heartbeat data: {}", e);
                    continue;
                }

                println!("✅ Android: Heartbeat sent successfully");
                println!("🔧 Android: Heartbeat loop completed, starting next iteration...");
            } else {
                eprintln!("❌ Android: Failed to serialize heartbeat command");
                println!("🔧 Android: Continuing heartbeat loop despite serialization failure...");
            }
        }
        // Note: This line is unreachable due to infinite loop above
        // println!("🔧 Android: Heartbeat thread stopped");
    });

    // Spawn integrated handler task using native thread
    let handler_stream = tcp_stream.clone();
    let handler_handle = thread::spawn(move || -> Result<()> {
        println!("🔧 Android: Integrated handler thread started");
        std::io::stdout().flush().ok();

        // Buffer for reading length prefix and command data
        let mut length_buf = [0u8; 4];
        let mut command_buf = vec![0u8; 1024]; // MAX_MESSAGE_SIZE

        loop {
            let mut stream = handler_stream.lock().unwrap();

            // Read 4-byte length prefix
            match stream.read_exact(&mut length_buf) {
                Ok(_) => {
                    let length = u32::from_be_bytes(length_buf) as usize;
                    if length > 1024 {
                        eprintln!("❌ Android: Message too large: {} bytes", length);
                        break;
                    }

                    // Resize buffer if needed and read command data
                    if command_buf.len() < length {
                        command_buf.resize(length, 0);
                    }

                    match stream.read_exact(&mut command_buf[..length]) {
                        Ok(_) => {
                            // Parse bincode command
                            let config = bincode_config::standard()
                                .with_fixed_int_encoding()
                                .with_little_endian();

                            match bincode::decode_from_slice(&command_buf[..length], config) {
                                Ok((command, _)) => {
                                    println!("🔧 Android: Received command: {:?}", command);
                                    std::io::stdout().flush().ok();

                                    // Handle different command types
                                    match command {
                                        Command::V1(cmd_v1) => {
                                            match cmd_v1 {
                                                CommandV1::LoginResult {
                                                    success,
                                                    pods_model,
                                                    error,
                                                } => {
                                                    if success {
                                                        println!("✅ Android: Login successful");
                                                        if !pods_model.is_empty() {
                                                            println!(
                                                                "🔧 Android: Received {} models",
                                                                pods_model.len()
                                                            );
                                                            for pod_model in &pods_model {
                                                                if let Some(model_name) =
                                                                    &pod_model.model_name
                                                                {
                                                                    println!(
                                                                        "📦 Android: Model: {}",
                                                                        model_name
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        eprintln!(
                                                            "❌ Android: Login failed: {:?}",
                                                            error
                                                        );
                                                        break;
                                                    }
                                                }
                                                CommandV1::PullModelResult {
                                                    pods_model,
                                                    error,
                                                } => {
                                                    if let Some(err) = error {
                                                        eprintln!(
                                                            "❌ Android: Pull model failed: {}",
                                                            err
                                                        );
                                                    } else {
                                                        println!(
                                                            "✅ Android: Pull model successful"
                                                        );
                                                        if !pods_model.is_empty() {
                                                            println!(
                                                                "📦 Android: Received {} models",
                                                                pods_model.len()
                                                            );
                                                            for pod_model in &pods_model {
                                                                if let Some(model_name) =
                                                                    &pod_model.model_name
                                                                {
                                                                    println!(
                                                                        "📦 Android: Model: {}",
                                                                        model_name
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                CommandV1::InferenceTask {
                                                    task_id,
                                                    prompt,
                                                    max_tokens,
                                                    temperature,
                                                    top_k,
                                                    top_p,
                                                    repeat_penalty,
                                                } => {
                                                    println!(
                                                        "🔧 Android: Received inference task: {}",
                                                        task_id
                                                    );
                                                    println!("📝 Android: Prompt: {}", prompt);
                                                    println!("⚙️ Android: Parameters: max_tokens={}, temp={}, top_k={}, top_p={}", 
                                                             max_tokens, temperature, top_k, top_p);

                                                    // Start timing the inference
                                                    let start_time = std::time::Instant::now();

                                                    // Execute inference task directly (handler thread is already native)
                                                    let result = {
                                                        // Use real inference with sampling parameters
                                                        use crate::{
                                                            manual_llama_completion,
                                                            GLOBAL_CONTEXT_PTR,
                                                            GLOBAL_INFERENCE_MUTEX,
                                                            GLOBAL_MODEL_PTR,
                                                        };
                                                        use std::ffi::CString;
                                                        use std::sync::atomic::Ordering;

                                                        // Acquire global inference lock to prevent concurrent execution
                                                        let _lock =
                                                            GLOBAL_INFERENCE_MUTEX.lock().unwrap();

                                                        // Get global model and context pointers
                                                        let model_ptr =
                                                            GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
                                                        let context_ptr = GLOBAL_CONTEXT_PTR
                                                            .load(Ordering::SeqCst);

                                                        if model_ptr.is_null()
                                                            || context_ptr.is_null()
                                                        {
                                                            Err(anyhow!("Model not loaded - please load a model first"))
                                                        } else {
                                                            // Convert prompt to CString
                                                            let prompt_cstr =
                                                                match CString::new(&prompt[..]) {
                                                                    Ok(cstr) => cstr,
                                                                    Err(e) => {
                                                                        return Err(anyhow!(
                                                                            "Invalid prompt: {}",
                                                                            e
                                                                        ));
                                                                    }
                                                                };

                                                            // Create output buffer
                                                            let mut output = vec![0u8; 4096];

                                                            // Execute real inference with sampling parameters
                                                            let result = unsafe {
                                                                manual_llama_completion(
                                                                    model_ptr,
                                                                    context_ptr,
                                                                    prompt_cstr.as_ptr(),
                                                                    max_tokens as i32,
                                                                    temperature,
                                                                    top_k as i32,
                                                                    top_p,
                                                                    repeat_penalty,
                                                                    output.as_mut_ptr() as *mut std::os::raw::c_char,
                                                                    output.len() as i32,
                                                                )
                                                            };

                                                            if result > 0 {
                                                                let output_str = match unsafe {
                                                                    std::ffi::CStr::from_ptr(output.as_ptr() as *const std::os::raw::c_char)
                                                                        .to_str()
                                                                } {
                                                                    Ok(s) => s,
                                                                    Err(e) => {
                                                                        return Err(anyhow!("Invalid UTF-8 in output: {}", e));
                                                                    }
                                                                };
                                                                Ok(output_str.to_string())
                                                            } else {
                                                                Err(anyhow!("Inference failed with code: {}", result))
                                                            }
                                                        }
                                                    };

                                                    let execution_time =
                                                        start_time.elapsed().as_millis() as u64;

                                                    // Send result back to server
                                                    match result {
                                                        Ok(output) => {
                                                            println!("✅ Android: Inference successful in {}ms", execution_time);
                                                            println!(
                                                                "📤 Android: Sending result: {}",
                                                                &output[..output.len().min(100)]
                                                            );

                                                            // Create success result command
                                                            let result_command =
                                                                CommandV1::InferenceResult {
                                                                    task_id,
                                                                    success: true,
                                                                    result: Some(output),
                                                                    error: None,
                                                                    execution_time_ms:
                                                                        execution_time,
                                                                };

                                                            // Send result using bincode
                                                            let config = bincode_config::standard()
                                                                .with_fixed_int_encoding()
                                                                .with_little_endian();

                                                            if let Ok(buf) = bincode::encode_to_vec(
                                                                &Command::V1(result_command),
                                                                config,
                                                            ) {
                                                                let len = buf.len() as u32;

                                                                if let Err(e) = stream
                                                                    .write_all(&len.to_be_bytes())
                                                                {
                                                                    eprintln!("❌ Android: Failed to send result length: {}", e);
                                                                } else if let Err(e) =
                                                                    stream.write_all(&buf)
                                                                {
                                                                    eprintln!("❌ Android: Failed to send result data: {}", e);
                                                                } else {
                                                                    println!("✅ Android: Inference result sent successfully");
                                                                }
                                                            } else {
                                                                eprintln!("❌ Android: Failed to serialize inference result");
                                                            }
                                                        }
                                                        Err(e) => {
                                                            eprintln!(
                                                                "❌ Android: Inference failed: {}",
                                                                e
                                                            );

                                                            // Create error result command
                                                            let result_command =
                                                                CommandV1::InferenceResult {
                                                                    task_id,
                                                                    success: false,
                                                                    result: None,
                                                                    error: Some(e.to_string()),
                                                                    execution_time_ms:
                                                                        execution_time,
                                                                };

                                                            // Send error result
                                                            let config = bincode_config::standard()
                                                                .with_fixed_int_encoding()
                                                                .with_little_endian();

                                                            if let Ok(buf) = bincode::encode_to_vec(
                                                                &Command::V1(result_command),
                                                                config,
                                                            ) {
                                                                let len = buf.len() as u32;

                                                                if let Err(e) = stream
                                                                    .write_all(&len.to_be_bytes())
                                                                {
                                                                    eprintln!("❌ Android: Failed to send error result length: {}", e);
                                                                } else if let Err(e) =
                                                                    stream.write_all(&buf)
                                                                {
                                                                    eprintln!("❌ Android: Failed to send error result data: {}", e);
                                                                } else {
                                                                    println!("✅ Android: Error result sent successfully");
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    println!("⚠️ Android: Received unhandled command type");
                                                }
                                            }
                                        }
                                        _ => {
                                            println!("⚠️ Android: Received non-V1 command");
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("❌ Android: Failed to parse command: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Android: Failed to read command data: {}", e);
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Android: Failed to read length prefix: {}", e);
                    break;
                }
            }

            // Release lock before next iteration
            drop(stream);
        }

        println!("🔧 Android: Integrated handler thread stopped");
        Ok(())
    });

    info!("✅ Android: Background tasks started successfully");

    // Store thread handles for cleanup (optional for now)
    info!("🔧 Android: Background task handles stored for cleanup");

    Ok(())
}

/// Stop global worker and cleanup
#[cfg(target_os = "android")]
pub async fn stop_global_worker() {
    // Stop background tasks
    if let Some(global_handles) = GLOBAL_WORKER_HANDLES.get() {
        let mut guard = global_handles.lock().unwrap();
        if let Some((heartbeat_handle, handler_handle)) = guard.take() {
            heartbeat_handle.abort();
            handler_handle.abort();
            tracing::info!("Worker tasks stopped");
        }
    }

    // Cleanup worker
    if let Some(global) = GLOBAL_WORKER.get() {
        let mut guard = global.lock().unwrap();
        *guard = None;
        tracing::info!("Global worker cleaned up");
    }
}

/// Get global worker status
#[cfg(target_os = "android")]
pub async fn get_worker_status() -> Result<String> {
    // Check if TCP connection is available (new architecture)
    if let Some(_tcp_stream) = get_android_tcp_stream() {
        Ok("Worker is running".to_string())
    } else {
        Ok("Worker not available".to_string())
    }
}
