//! Android-specific login implementation using native threads
//!
//! This module provides Android-compatible login functionality that avoids
//! the Tokio runtime issues in shell environments by using native threads
//! and blocking I/O operations.

#[cfg(target_os = "android")]
use anyhow::{anyhow, Result};

#[cfg(target_os = "android")]
use common::{ChatMessage, Command, CommandV1, Model, OsType, SystemInfo};

#[cfg(target_os = "android")]
use std::ffi::{CStr, CString};

#[cfg(target_os = "android")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(target_os = "android")]
use super::{Args, AutoWorker, WorkerHandle};
#[cfg(target_os = "android")]
use common::{DevicesInfo, EngineType};
#[cfg(target_os = "android")]
use std::io::Write;
#[cfg(target_os = "android")]
use tokio::task::JoinHandle;
#[cfg(target_os = "android")]
use tracing::info;

#[cfg(target_os = "android")]
use std::time::Duration;

#[cfg(target_os = "android")]
fn build_chat_prompt(messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    for m in messages {
        let role = m.role.as_str();
        match role {
            "system" => {
                out.push_str("System: ");
                out.push_str(&m.content);
                out.push('\n');
            }
            "user" => {
                out.push_str("User: ");
                out.push_str(&m.content);
                out.push('\n');
            }
            "assistant" => {
                out.push_str("Assistant: ");
                out.push_str(&m.content);
                out.push('\n');
            }
            _ => {
                out.push_str(role);
                out.push_str(": ");
                out.push_str(&m.content);
                out.push('\n');
            }
        }
    }
    out.push_str("Assistant: ");
    out
}

#[cfg(target_os = "android")]
extern "C" {
    fn llama_get_model(ctx: *const crate::llama_context) -> *const crate::llama_model;
    fn llama_model_get_vocab(model: *const crate::llama_model) -> *const crate::llama_vocab;
    fn llama_tokenize(
        vocab: *const crate::llama_vocab,
        text: *const std::os::raw::c_char,
        text_len: i32,
        tokens: *mut i32,
        n_tokens_max: i32,
        add_special: bool,
        parse_special: bool,
    ) -> i32;
    fn llama_model_meta_val_str(
        model: *const crate::llama_model,
        key: *const std::os::raw::c_char,
        buf: *mut std::os::raw::c_char,
        buf_size: usize,
    ) -> i32;
    fn llama_chat_apply_template(
        tmpl: *const std::os::raw::c_char,
        chat: *const crate::llama_chat_message,
        n_msg: usize,
        add_ass: bool,
        buf: *mut std::os::raw::c_char,
        length: i32,
    ) -> i32;
}

#[cfg(target_os = "android")]
fn count_prompt_tokens(ctx: *mut crate::llama_context, prompt: *const std::os::raw::c_char) -> u32 {
    if ctx.is_null() || prompt.is_null() {
        return 0;
    }

    let model = unsafe { llama_get_model(ctx) };
    if model.is_null() {
        return 0;
    }
    let vocab = unsafe { llama_model_get_vocab(model) };
    if vocab.is_null() {
        return 0;
    }

    let prompt_len = unsafe { std::ffi::CStr::from_ptr(prompt) }.to_bytes().len();

    let mut tokens: Vec<i32> = vec![0; 512];
    let mut n = unsafe {
        llama_tokenize(
            vocab,
            prompt,
            prompt_len as i32,
            tokens.as_mut_ptr(),
            tokens.len() as i32,
            true,
            true,
        )
    };
    if n < 0 {
        let needed = (-n) as usize;
        tokens = vec![0; needed.max(1)];
        n = unsafe {
            llama_tokenize(
                vocab,
                prompt,
                prompt_len as i32,
                tokens.as_mut_ptr(),
                tokens.len() as i32,
                true,
                true,
            )
        };
    }

    if n > 0 { n as u32 } else { 0 }
}

#[cfg(target_os = "android")]
fn build_chat_prompt_with_gguf_template(
    ctx: *mut crate::llama_context,
    messages: &[ChatMessage],
) -> Option<String> {
    if ctx.is_null() {
        return None;
    }

    let model = unsafe { llama_get_model(ctx) };
    if model.is_null() {
        return None;
    }

    let key = CString::new("tokenizer.chat_template").ok()?;
    let mut tmpl_buf = vec![0u8; 8192];
    let got = unsafe {
        llama_model_meta_val_str(
            model,
            key.as_ptr(),
            tmpl_buf.as_mut_ptr() as *mut std::os::raw::c_char,
            tmpl_buf.len(),
        )
    };
    if got <= 0 {
        return None;
    }

    let tmpl_cstr = unsafe { CStr::from_ptr(tmpl_buf.as_ptr() as *const std::os::raw::c_char) };

    let mut role_cstrs = Vec::with_capacity(messages.len());
    let mut content_cstrs = Vec::with_capacity(messages.len());
    let mut chat_msgs = Vec::with_capacity(messages.len());

    for m in messages {
        let role = CString::new(m.role.as_str()).ok()?;
        let content = CString::new(m.content.as_str()).ok()?;
        chat_msgs.push(crate::llama_chat_message {
            role: role.as_ptr(),
            content: content.as_ptr(),
        });
        role_cstrs.push(role);
        content_cstrs.push(content);
    }

    let needed = unsafe {
        llama_chat_apply_template(
            tmpl_cstr.as_ptr(),
            chat_msgs.as_ptr(),
            chat_msgs.len(),
            true,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 {
        return None;
    }

    let mut out = vec![0u8; needed as usize + 1];
    let written = unsafe {
        llama_chat_apply_template(
            tmpl_cstr.as_ptr(),
            chat_msgs.as_ptr(),
            chat_msgs.len(),
            true,
            out.as_mut_ptr() as *mut std::os::raw::c_char,
            out.len() as i32,
        )
    };
    if written <= 0 {
        return None;
    }

    let s = unsafe { CStr::from_ptr(out.as_ptr() as *const std::os::raw::c_char) }
        .to_string_lossy()
        .to_string();
    Some(s)
}

#[cfg(target_os = "android")]
fn token_should_be_suppressed(token: &str) -> bool {
    token.contains("<|")
        || token.contains("<assistant")
        || token.contains("assistantfinal")
        || token.contains("analysis")
        || token.contains("The user is speaking")
        || token.contains("The assistant should")
        || token.contains("We need to")
        || token.contains("Thus produce")
        || token.contains("Ok produce answer")
}

#[cfg(target_os = "android")]
fn derive_model_id_from_path(model_path: &str) -> String {
    let lower = model_path.to_ascii_lowercase();
    if lower.contains("llama-3") || lower.contains("llama3") {
        return "llama3".to_string();
    }

    let file_name = std::path::Path::new(model_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(model_path);

    file_name
        .trim_end_matches(".gguf")
        .trim_end_matches(".bin")
        .to_string()
}

/// Get real-time system usage information for heartbeat
#[cfg(target_os = "android")]
fn get_realtime_system_usage() -> (u32, u32, u32) {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    // Get CPU usage from /proc/stat
    let cpu_usage = if let Ok(file) = File::open("/proc/stat") {
        let reader = BufReader::new(file);
        let mut cpu_percent = 25u32; // fallback

        for line in reader.lines().flatten() {
            if line.starts_with("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 8 {
                    // Parse CPU times: user, nice, system, idle, iowait, irq, softirq, steal
                    let mut times = Vec::new();
                    for i in 1..8 {
                        if let Ok(time) = parts[i].parse::<u64>() {
                            times.push(time);
                        }
                    }

                    if times.len() >= 4 {
                        let total_time: u64 = times.iter().sum();
                        let idle_time = times[3]; // idle time is the 4th value

                        if total_time > 0 {
                            cpu_percent = ((total_time - idle_time) * 100 / total_time) as u32;
                        }
                    }
                }
                break;
            }
        }
        cpu_percent
    } else {
        25 // fallback
    };

    // Get memory usage from /proc/meminfo
    let memory_usage = if let Ok(file) = File::open("/proc/meminfo") {
        let reader = BufReader::new(file);
        let mut total_memory = 0u64;
        let mut available_memory = 0u64;

        for line in reader.lines().flatten() {
            if line.starts_with("MemTotal:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(mem_kb) = parts[1].parse::<u64>() {
                        total_memory = mem_kb;
                    }
                }
            } else if line.starts_with("MemAvailable:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(mem_kb) = parts[1].parse::<u64>() {
                        available_memory = mem_kb;
                    }
                }
            }
        }

        if total_memory > 0 && available_memory > 0 {
            let used_memory = total_memory - available_memory;
            ((used_memory * 100) / total_memory) as u32
        } else {
            45 // fallback
        }
    } else {
        45 // fallback
    };

    // Get real disk usage using statvfs syscall
    let disk_usage = read_disk_usage().unwrap_or(60);

    (cpu_usage, memory_usage, disk_usage)
}

/// Global network usage tracking for incremental calculation
static LAST_NETWORK_USAGE: OnceLock<(u64, u64)> = OnceLock::new();

/// Get network usage information (incremental since last heartbeat)
#[cfg(target_os = "android")]
fn get_network_usage() -> (u64, u64) {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let mut current_rx = 0u64;
    let mut current_tx = 0u64;

    // Read current network statistics
    if let Ok(file) = File::open("/proc/net/dev") {
        let reader = BufReader::new(file);

        for line in reader.lines().flatten() {
            // Skip header lines
            if line.contains("Inter-") || line.contains("face") {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 17 {
                let interface_name = parts[0].trim_end_matches(':');

                // Skip loopback and virtual interfaces
                if interface_name == "lo"
                    || interface_name.starts_with("dummy")
                    || interface_name.starts_with("virbr")
                    || interface_name.starts_with("docker")
                {
                    continue;
                }

                // Parse rx and tx bytes (columns 1 and 9)
                if let (Ok(rx_bytes), Ok(tx_bytes)) =
                    (parts[1].parse::<u64>(), parts[9].parse::<u64>())
                {
                    current_rx += rx_bytes;
                    current_tx += tx_bytes;
                }
            }
        }
    }

    // Get last recorded values
    let (last_rx, last_tx) = LAST_NETWORK_USAGE
        .get()
        .copied()
        .unwrap_or((current_rx, current_tx));

    // Update last values for next call
    let _ = LAST_NETWORK_USAGE.set((current_rx, current_tx));

    // Calculate incremental usage (in bytes)
    let incremental_rx = if current_rx >= last_rx {
        current_rx - last_rx
    } else {
        0
    };
    let incremental_tx = if current_tx >= last_tx {
        current_tx - last_tx
    } else {
        0
    };

    // Return raw bytes for more precise network monitoring
    (incremental_rx, incremental_tx)
}

/// Read disk usage percentage using statvfs syscall
#[cfg(target_os = "android")]
fn read_disk_usage() -> Option<u32> {
    use std::ffi::CString;
    use std::mem;

    // Try to get disk usage for the data partition
    let path = CString::new("/data").ok()?;

    // statvfs structure for Android
    #[repr(C)]
    struct Statvfs {
        f_bsize: u64,   // File system block size
        f_frsize: u64,  // Fragment size
        f_blocks: u64,  // Total blocks
        f_bfree: u64,   // Free blocks
        f_bavail: u64,  // Available blocks
        f_files: u64,   // Total file nodes
        f_ffree: u64,   // Free file nodes
        f_favail: u64,  // Available file nodes
        f_fsid: u64,    // File system ID
        f_flag: u64,    // Mount flags
        f_namemax: u64, // Maximum filename length
    }

    extern "C" {
        fn statvfs(path: *const std::os::raw::c_char, buf: *mut Statvfs) -> std::os::raw::c_int;
    }

    let mut stat = unsafe { mem::zeroed::<Statvfs>() };
    let result = unsafe { statvfs(path.as_ptr(), &mut stat) };

    if result == 0 {
        if stat.f_blocks > 0 {
            let used_blocks = stat.f_blocks - stat.f_bavail;
            let usage_percent = (used_blocks * 100) / stat.f_blocks;
            Some(usage_percent as u32)
        } else {
            None
        }
    } else {
        None
    }
}
/// Global TCP connection storage for Android background tasks
pub static ANDROID_TCP_STREAM: OnceLock<Mutex<Option<Arc<Mutex<std::net::TcpStream>>>>> =
    OnceLock::new();

/// Global server address storage for creating separate connections
pub static ANDROID_SERVER_ADDR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// Global control port storage for heartbeat connections
pub static ANDROID_CONTROL_PORT: OnceLock<Mutex<Option<u16>>> = OnceLock::new();

/// Global client_id storage for Android background tasks
pub static ANDROID_CLIENT_ID: OnceLock<Mutex<Option<[u8; 16]>>> = OnceLock::new();

#[cfg(target_os = "android")]
static ANDROID_ACTIVE_TASK_ID: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(target_os = "android")]
/// Global worker task handle for background operations
static GLOBAL_WORKER_HANDLES: OnceLock<
    Mutex<
        Option<(
            std::thread::JoinHandle<()>,
            std::thread::JoinHandle<Result<()>>,
        )>,
    >,
> = OnceLock::new();

/// Global stop signal for background threads
#[cfg(target_os = "android")]
static GLOBAL_STOP_SIGNAL: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Perform Android-native login using blocking TCP and bincode protocol
///
/// This function replicates the functionality of TCPWorker::login() but
/// uses native threads and blocking I/O to avoid Tokio runtime issues
/// in Android shell environments.
///
#[cfg(target_os = "android")]
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

    let _ = ANDROID_ACTIVE_TASK_ID.set(Mutex::new(None));

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
        cpu_usage: cpu_usage as u8,
        memory_usage: memory_usage as u8,
        disk_usage: disk_usage as u8,
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

    // Send login command using common library function
    info!("📤 Android: Sending login command...");
    common::write_command_sync(&mut stream, &Command::V1(login_cmd))
        .map_err(|e| anyhow!("Failed to send login command: {}", e))?;

    info!("✅ Android: Login command sent successfully");

    // Store TCP connection globally for background tasks
    let stream_arc = Arc::new(Mutex::new(stream));
    {
        let slot = ANDROID_TCP_STREAM.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(stream_arc.clone());
    }

    // Store server address for heartbeat connections (only IP, without port)
    {
        let slot = ANDROID_SERVER_ADDR.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(server_addr.to_string());
    }

    // Store control port for heartbeat connections
    {
        let slot = ANDROID_CONTROL_PORT.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(control_port);
    }

    // Store client_id globally for background tasks
    let client_id_bytes = hex::decode(client_id)
        .unwrap_or_default()
        .try_into()
        .unwrap_or_default();
    {
        let slot = ANDROID_CLIENT_ID.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(client_id_bytes);
    }

    info!("✅ Android: TCP connection, server address, and client_id stored for background tasks");

    Ok(())
}

/// Get the stored TCP connection for background tasks
pub fn get_android_tcp_stream() -> Option<Arc<Mutex<std::net::TcpStream>>> {
    ANDROID_TCP_STREAM
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
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

    info!("Global worker initialized successfully");
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

    // Initialize stop signal
    let stop_signal = Arc::new(AtomicBool::new(false));
    GLOBAL_STOP_SIGNAL
        .set(stop_signal.clone())
        .map_err(|_| anyhow!("Failed to set stop signal"))?;

    // Spawn heartbeat task using native thread with full heartbeat logic
    let heartbeat_stream = tcp_stream.clone();
    let heartbeat_stop_signal = stop_signal.clone();
    let heartbeat_handle = thread::spawn(move || {
        println!("🔧 Android: Heartbeat thread started");

        loop {
            // Check stop signal before sleeping
            if heartbeat_stop_signal.load(Ordering::Relaxed) {
                println!("🔧 Android: Heartbeat thread received stop signal");
                break;
            }
            println!("🔧 Android: Heartbeat loop - sleeping for 120 seconds...");

            println!("💓 Android: Woke up - collecting system info for heartbeat...");

            // Use simple static values for system info to avoid async issues
            let cpu_usage = 25; // Static CPU usage percentage
            let memory_usage = 45; // Static memory usage percentage
            let disk_usage = 60; // Static disk usage percentage
            let network_rx = 0; // Static network RX
            let network_tx = 0; // Static network TX

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
                "💓 Android: Sending heartbeat - CPU: {}% MEM: {}% DISK: {}% NET: ↑{}B ↓{}B MEM_TOTAL: {}GB",
                cpu_usage, memory_usage, disk_usage, network_tx, network_rx, device_info.memtotal_gb
            );

            // Send heartbeat using independent TCP connection to avoid lock conflicts
            let server_addr = match ANDROID_SERVER_ADDR
                .get()
                .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
            {
                Some(addr) => addr,
                None => {
                    eprintln!("❌ Android: Server address not stored, skipping heartbeat");
                    continue;
                }
            };

            // Create new connection for each heartbeat to avoid conflicts
            let mut heartbeat_stream = match std::net::TcpStream::connect(server_addr) {
                Ok(stream) => {
                    println!("✅ Android: Connected to server for heartbeat");
                    stream
                }
                Err(e) => {
                    eprintln!("❌ Android: Failed to connect for heartbeat: {}", e);
                    continue;
                }
            };

            // Create heartbeat command
            let client_id = ANDROID_CLIENT_ID
                .get()
                .and_then(|m| m.lock().ok().and_then(|g| *g))
                .unwrap_or([0u8; 16]);
            let heartbeat_cmd = CommandV1::Heartbeat {
                client_id,
                system_info: SystemInfo {
                    cpu_usage: cpu_usage as u8,
                    memory_usage: memory_usage as u8,
                    disk_usage: disk_usage as u8,
                    network_rx: 0, // TODO: Implement network monitoring
                    network_tx: 0,
                },
                device_memtotal_gb: device_info.memtotal_gb.try_into().unwrap_or(0),
                device_total_tflops: device_info.total_tflops.into(),
                device_count: device_info.num as u16,
                devices_info: vec![device_info],
            };

            // Send heartbeat using common library function
            if let Err(e) =
                common::write_command_sync(&mut heartbeat_stream, &Command::V1(heartbeat_cmd))
            {
                eprintln!("❌ Android: Failed to send heartbeat: {}", e);
                println!("🔧 Android: Continuing heartbeat loop despite send failure...");
            } else {
                println!("✅ Android: Heartbeat sent successfully");
            }

            // Close the connection after sending heartbeat
            drop(heartbeat_stream);
            println!("🔧 Android: Heartbeat connection closed, starting next iteration...");

            // Sleep with periodic stop signal checks
            for _ in 0..120 {
                // 120 seconds / 1 second intervals
                thread::sleep(Duration::from_secs(1));
                if heartbeat_stop_signal.load(Ordering::Relaxed) {
                    println!("🔧 Android: Heartbeat thread received stop signal during sleep");
                    break;
                }
            }

            // Check stop signal after sleep
            if heartbeat_stop_signal.load(Ordering::Relaxed) {
                println!("🔧 Android: Heartbeat thread received stop signal after sleep");
                break;
            }
        }
        println!("🔧 Android: Heartbeat thread stopped");
    });

    // Spawn integrated handler task using native thread
    let handler_stream = tcp_stream.clone();
    let handler_stop_signal = stop_signal.clone();
    let handler_handle = thread::spawn(move || -> Result<()> {
        println!("🔧 Android: Integrated handler thread started");
        std::io::stdout().flush().ok();

        loop {
            // Check stop signal before processing
            if handler_stop_signal.load(Ordering::Relaxed) {
                println!("🔧 Android: Handler thread received stop signal");
                break;
            }

            let mut stream = handler_stream.lock().unwrap();

            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));

            // Read command using common library function
            match common::read_command_sync(&mut *stream) {
                Ok(command) => {
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
                                        let client_id = ANDROID_CLIENT_ID
                                            .get()
                                            .and_then(|m| m.lock().ok().and_then(|g| *g))
                                            .unwrap_or([0u8; 16]);
                                        let current_model_path = crate::MODEL_STATUS
                                            .lock()
                                            .ok()
                                            .and_then(|s| s.current_model.clone())
                                            .unwrap_or_else(|| "android".to_string());
                                        let model_id =
                                            derive_model_id_from_path(&current_model_path);
                                        println!("model_id: {}", model_id);
                                        let models = vec![Model {
                                            id: model_id,
                                            object: "model".to_string(),
                                            created: 0,
                                            owned_by: "android".to_string(),
                                        }];
                                        let model_status = CommandV1::ModelStatus {
                                            client_id,
                                            models,
                                            auto_models_device: Vec::new(),
                                        };
                                        let _ = common::write_command_sync(
                                            &mut *stream,
                                            &Command::V1(model_status),
                                        );
                                        if !pods_model.is_empty() {
                                            println!(
                                                "🔧 Android: Received {} models",
                                                pods_model.len()
                                            );
                                            for pod_model in &pods_model {
                                                if let Some(model_name) = &pod_model.model_name {
                                                    println!("📦 Android: Model: {}", model_name);
                                                }
                                            }
                                        }
                                    } else {
                                        eprintln!("❌ Android: Login failed: {:?}", error);
                                        break;
                                    }
                                }
                                CommandV1::PullModelResult { pods_model, error } => {
                                    if let Some(err) = error {
                                        eprintln!("❌ Android: Pull model failed: {}", err);
                                    } else {
                                        println!("✅ Android: Pull model successful");
                                        if !pods_model.is_empty() {
                                            println!(
                                                "📦 Android: Received {} models",
                                                pods_model.len()
                                            );
                                            for pod_model in &pods_model {
                                                if let Some(model_name) = &pod_model.model_name {
                                                    println!("📦 Android: Model: {}", model_name);
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
                                    repeat_last_n: _,
                                    min_keep: _,
                                } => {
                                    println!("🔧 Android: Received inference task: {}", task_id);
                                    println!("📝 Android: Prompt: {}", prompt);
                                    println!("⚙️ Android: Parameters: max_tokens={}, temp={}, top_k={}, top_p={}", 
                                                             max_tokens, temperature, top_k, top_p);

                                    use crate::llama_context;
                                    use crate::{
                                        gpuf_start_generation_async, GLOBAL_CONTEXT_PTR,
                                        GLOBAL_INFERENCE_MUTEX,
                                    };
                                    use std::ffi::CString;
                                    use std::os::raw::c_void;
                                    let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);
                                    if context_ptr.is_null() {
                                        let result_command = CommandV1::InferenceResultChunk {
                                            task_id: task_id.clone(),
                                            seq: 0,
                                            delta: String::new(),
                                            done: true,
                                            error: Some(
                                                "Model not loaded - please load a model first"
                                                    .to_string(),
                                            ),
                                            prompt_tokens: 0,
                                            completion_tokens: 0,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut *stream,
                                            &Command::V1(result_command),
                                        );
                                        continue;
                                    }

                                    {
                                        let mut active = ANDROID_ACTIVE_TASK_ID
                                            .get()
                                            .expect("ANDROID_ACTIVE_TASK_ID is set")
                                            .lock()
                                            .unwrap();
                                        *active = Some(task_id.clone());
                                    }

                                    let writer_stream = match stream.try_clone() {
                                        Ok(s) => s,
                                        Err(e) => {
                                            let err = format!("Failed to clone TCP stream: {}", e);
                                            let result_command = CommandV1::InferenceResultChunk {
                                                task_id: task_id.clone(),
                                                seq: 0,
                                                delta: String::new(),
                                                done: true,
                                                error: Some(err.clone()),
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            let _ = common::write_command_sync(
                                                &mut *stream,
                                                &Command::V1(result_command),
                                            );
                                            continue;
                                        }
                                    };

                                    let task_id_for_thread = task_id.clone();
                                    let prompt_for_thread = prompt.clone();
                                    let context_ptr_usize = context_ptr as usize;
                                    std::thread::spawn(move || {
                                        let context_ptr = context_ptr_usize as *mut llama_context;
                                        #[repr(C)]
                                        struct TokenCallbackState {
                                            stream: std::net::TcpStream,
                                            task_id: String,
                                            seq: u32,
                                            buf: String,
                                            max_bytes: usize,
                                            prompt_tokens: u32,
                                            completion_tokens: u32,
                                            suppress: bool,
                                        }

                                        extern "C" fn on_token(
                                            token: *const std::os::raw::c_char,
                                            user_data: *mut c_void,
                                        ) {
                                            if token.is_null() || user_data.is_null() {
                                                return;
                                            }

                                            let state = unsafe {
                                                &mut *(user_data as *mut TokenCallbackState)
                                            };
                                            let token_str =
                                                unsafe { std::ffi::CStr::from_ptr(token).to_str() };
                                            let Ok(token_str) = token_str else {
                                                return;
                                            };

                                            if state.suppress {
                                                if token_str.contains("assistantfinal")
                                                    || token_str.contains("final")
                                                {
                                                    state.suppress = false;
                                                }
                                                return;
                                            }

                                            if token_should_be_suppressed(token_str)
                                                && state.completion_tokens < 128
                                            {
                                                state.suppress = true;
                                                state.buf.clear();
                                                return;
                                            }

                                            state.buf.push_str(token_str);
                                            state.completion_tokens =
                                                state.completion_tokens.saturating_add(1);
                                            if state.buf.len() < state.max_bytes {
                                                return;
                                            }

                                            let delta = std::mem::take(&mut state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: state.task_id.clone(),
                                                seq: state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: state.prompt_tokens,
                                                completion_tokens: state.completion_tokens,
                                            };
                                            state.seq = state.seq.wrapping_add(1);

                                            let _ = common::write_command_sync(
                                                &mut state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();
                                        let start_time = std::time::Instant::now();
                                        let prompt_cstr = match CString::new(prompt_for_thread) {
                                            Ok(s) => s,
                                            Err(e) => {
                                                let err = format!("Invalid prompt: {}", e);
                                                let mut s = writer_stream;
                                                let result_command =
                                                    CommandV1::InferenceResultChunk {
                                                        task_id: task_id_for_thread.clone(),
                                                        seq: 0,
                                                        delta: String::new(),
                                                        done: true,
                                                        error: Some(err),
                                                        prompt_tokens: 0,
                                                        completion_tokens: 0,
                                                    };
                                                let _ = common::write_command_sync(
                                                    &mut s,
                                                    &Command::V1(result_command),
                                                );
                                                return;
                                            }
                                        };

                                        let prompt_tokens: u32 =
                                            count_prompt_tokens(context_ptr, prompt_cstr.as_ptr());

                                        let mut cb_state = TokenCallbackState {
                                            stream: writer_stream,
                                            task_id: task_id_for_thread.clone(),
                                            seq: 0,
                                            buf: String::new(),
                                            max_bytes: 8,
                                            prompt_tokens,
                                            completion_tokens: 0,
                                            suppress: false,
                                        };

                                        let completion_tokens_i32 = unsafe {
                                            gpuf_start_generation_async(
                                                context_ptr,
                                                prompt_cstr.as_ptr(),
                                                max_tokens as i32,
                                                temperature,
                                                top_k as i32,
                                                top_p,
                                                repeat_penalty,
                                                Some(on_token),
                                                (&mut cb_state as *mut TokenCallbackState)
                                                    as *mut c_void,
                                            )
                                        };

                                        if completion_tokens_i32 > 0 {
                                            cb_state.completion_tokens = completion_tokens_i32 as u32;
                                        }

                                        if !cb_state.buf.is_empty() {
                                            let delta = std::mem::take(&mut cb_state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: task_id_for_thread.clone(),
                                                seq: cb_state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: cb_state.prompt_tokens,
                                                completion_tokens: cb_state.completion_tokens,
                                            };
                                            cb_state.seq = cb_state.seq.wrapping_add(1);
                                            let _ = common::write_command_sync(
                                                &mut cb_state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let done_chunk = CommandV1::InferenceResultChunk {
                                            task_id: task_id_for_thread.clone(),
                                            seq: cb_state.seq,
                                            delta: String::new(),
                                            done: true,
                                            error: None,
                                            prompt_tokens: cb_state.prompt_tokens,
                                            completion_tokens: cb_state.completion_tokens,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut cb_state.stream,
                                            &Command::V1(done_chunk),
                                        );

                                        {
                                            let mut active = ANDROID_ACTIVE_TASK_ID
                                                .get()
                                                .expect("ANDROID_ACTIVE_TASK_ID is set")
                                                .lock()
                                                .unwrap();
                                            if active.as_deref()
                                                == Some(task_id_for_thread.as_str())
                                            {
                                                *active = None;
                                            }
                                        }

                                        let execution_time =
                                            start_time.elapsed().as_millis() as u64;
                                        println!(
                                            "✅ Android: Streaming inference finished in {}ms",
                                            execution_time
                                        );
                                    });
                                }
                                CommandV1::ChatInferenceTask {
                                    task_id,
                                    model: _,
                                    messages,
                                    max_tokens,
                                    temperature,
                                    top_k,
                                    top_p,
                                    repeat_penalty,
                                    repeat_last_n: _,
                                    min_keep: _,
                                } => {
                                    println!(
                                        "🔧 Android: Received chat inference task: {}",
                                        task_id
                                    );

                                    use crate::llama_context;
                                    use crate::{
                                        gpuf_start_generation_async, GLOBAL_CONTEXT_PTR,
                                        GLOBAL_INFERENCE_MUTEX,
                                    };
                                    use std::ffi::CString;
                                    use std::os::raw::c_void;
                                    let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);
                                    if context_ptr.is_null() {
                                        let result_command = CommandV1::InferenceResultChunk {
                                            task_id: task_id.clone(),
                                            seq: 0,
                                            delta: String::new(),
                                            done: true,
                                            error: Some(
                                                "Model not loaded - please load a model first"
                                                    .to_string(),
                                            ),
                                            prompt_tokens: 0,
                                            completion_tokens: 0,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut *stream,
                                            &Command::V1(result_command),
                                        );
                                        continue;
                                    }

                                    let prompt = build_chat_prompt_with_gguf_template(
                                        context_ptr,
                                        &messages,
                                    )
                                    .unwrap_or_else(|| build_chat_prompt(&messages));
                                    println!("📝 Android: Prompt: {}", prompt);

                                    {
                                        let mut active = ANDROID_ACTIVE_TASK_ID
                                            .get()
                                            .expect("ANDROID_ACTIVE_TASK_ID is set")
                                            .lock()
                                            .unwrap();
                                        *active = Some(task_id.clone());
                                    }

                                    let writer_stream = match stream.try_clone() {
                                        Ok(s) => s,
                                        Err(e) => {
                                            let err = format!("Failed to clone TCP stream: {}", e);
                                            let result_command = CommandV1::InferenceResultChunk {
                                                task_id: task_id.clone(),
                                                seq: 0,
                                                delta: String::new(),
                                                done: true,
                                                error: Some(err.clone()),
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            let _ = common::write_command_sync(
                                                &mut *stream,
                                                &Command::V1(result_command),
                                            );
                                            continue;
                                        }
                                    };

                                    let task_id_for_thread = task_id.clone();
                                    let prompt_for_thread = prompt.clone();
                                    let context_ptr_usize = context_ptr as usize;
                                    std::thread::spawn(move || {
                                        let context_ptr = context_ptr_usize as *mut llama_context;
                                        #[repr(C)]
                                        struct TokenCallbackState {
                                            stream: std::net::TcpStream,
                                            task_id: String,
                                            seq: u32,
                                            buf: String,
                                            max_bytes: usize,
                                            prompt_tokens: u32,
                                            completion_tokens: u32,
                                            suppress: bool,
                                        }

                                        extern "C" fn on_token(
                                            token: *const std::os::raw::c_char,
                                            user_data: *mut c_void,
                                        ) {
                                            if token.is_null() || user_data.is_null() {
                                                return;
                                            }

                                            let state = unsafe {
                                                &mut *(user_data as *mut TokenCallbackState)
                                            };
                                            let token_str =
                                                unsafe { std::ffi::CStr::from_ptr(token).to_str() };
                                            let Ok(token_str) = token_str else {
                                                return;
                                            };

                                            if state.suppress {
                                                if token_str.contains("assistantfinal")
                                                    || token_str.contains("final")
                                                {
                                                    state.suppress = false;
                                                }
                                                return;
                                            }

                                            if token_should_be_suppressed(token_str)
                                                && state.completion_tokens < 128
                                            {
                                                state.suppress = true;
                                                state.buf.clear();
                                                return;
                                            }

                                            state.buf.push_str(token_str);
                                            state.completion_tokens =
                                                state.completion_tokens.saturating_add(1);
                                            if state.buf.len() < state.max_bytes {
                                                return;
                                            }

                                            let delta = std::mem::take(&mut state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: state.task_id.clone(),
                                                seq: state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: state.prompt_tokens,
                                                completion_tokens: state.completion_tokens,
                                            };
                                            state.seq = state.seq.wrapping_add(1);

                                            let _ = common::write_command_sync(
                                                &mut state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();
                                        let prompt_cstr = match CString::new(prompt_for_thread) {
                                            Ok(s) => s,
                                            Err(e) => {
                                                let err = format!("Invalid prompt: {}", e);
                                                let mut s = writer_stream;
                                                let result_command =
                                                    CommandV1::InferenceResultChunk {
                                                        task_id: task_id_for_thread.clone(),
                                                        seq: 0,
                                                        delta: String::new(),
                                                        done: true,
                                                        error: Some(err),
                                                        prompt_tokens: 0,
                                                        completion_tokens: 0,
                                                    };
                                                let _ = common::write_command_sync(
                                                    &mut s,
                                                    &Command::V1(result_command),
                                                );
                                                return;
                                            }
                                        };

                                        let prompt_tokens: u32 =
                                            count_prompt_tokens(context_ptr, prompt_cstr.as_ptr());

                                        let mut cb_state = TokenCallbackState {
                                            stream: writer_stream,
                                            task_id: task_id_for_thread.clone(),
                                            seq: 0,
                                            buf: String::new(),
                                            max_bytes: 8,
                                            prompt_tokens,
                                            completion_tokens: 0,
                                            suppress: false,
                                        };

                                        let completion_tokens_i32 = unsafe {
                                            gpuf_start_generation_async(
                                                context_ptr,
                                                prompt_cstr.as_ptr(),
                                                max_tokens as i32,
                                                temperature,
                                                top_k as i32,
                                                top_p,
                                                repeat_penalty,
                                                Some(on_token),
                                                (&mut cb_state as *mut TokenCallbackState)
                                                    as *mut c_void,
                                            )
                                        };

                                        if completion_tokens_i32 > 0 {
                                            cb_state.completion_tokens = completion_tokens_i32 as u32;
                                        }

                                        if !cb_state.buf.is_empty() {
                                            let delta = std::mem::take(&mut cb_state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: task_id_for_thread.clone(),
                                                seq: cb_state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: cb_state.prompt_tokens,
                                                completion_tokens: cb_state.completion_tokens,
                                            };
                                            cb_state.seq = cb_state.seq.wrapping_add(1);
                                            let _ = common::write_command_sync(
                                                &mut cb_state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let done_chunk = CommandV1::InferenceResultChunk {
                                            task_id: task_id_for_thread.clone(),
                                            seq: cb_state.seq,
                                            delta: String::new(),
                                            done: true,
                                            error: None,
                                            prompt_tokens: cb_state.prompt_tokens,
                                            completion_tokens: cb_state.completion_tokens,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut cb_state.stream,
                                            &Command::V1(done_chunk),
                                        );

                                        {
                                            let mut active = ANDROID_ACTIVE_TASK_ID
                                                .get()
                                                .expect("ANDROID_ACTIVE_TASK_ID is set")
                                                .lock()
                                                .unwrap();
                                            if active.as_deref()
                                                == Some(task_id_for_thread.as_str())
                                            {
                                                *active = None;
                                            }
                                        }
                                    });
                                }
                                CommandV1::CancelInference { task_id } => {
                                    let should_cancel = {
                                        let active_lock = ANDROID_ACTIVE_TASK_ID
                                            .get()
                                            .expect("ANDROID_ACTIVE_TASK_ID is set")
                                            .lock()
                                            .unwrap();
                                        active_lock.as_deref() == Some(task_id.as_str())
                                    };

                                    if should_cancel {
                                        use crate::{gpuf_stop_generation, GLOBAL_CONTEXT_PTR};
                                        let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);
                                        if !context_ptr.is_null() {
                                            unsafe { gpuf_stop_generation(context_ptr) };
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
                    // If the read timed out, loop again to re-check stop signal
                    if let Some(ioe) = e.downcast_ref::<std::io::Error>() {
                        if matches!(
                            ioe.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) {
                            drop(stream);
                            continue;
                        }
                    }
                    eprintln!("❌ Android: Error reading command: {}", e);
                    break;
                }
            }

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

/// Start background worker tasks with callback support (heartbeat, handler, etc.)
#[cfg(target_os = "android")]
pub async fn start_worker_tasks_with_callback_ptr(
    callback: Option<extern "C" fn(*const std::ffi::c_char, *mut std::ffi::c_void)>,
) -> Result<()> {
    use std::ffi::CString;
    use std::thread;

    info!("🔧 Android: Starting background tasks with native threads and callback...");

    // Get or initialize stop signal
    let stop_signal = if let Some(existing_signal) = GLOBAL_STOP_SIGNAL.get() {
        existing_signal.clone()
    } else {
        let new_signal = Arc::new(AtomicBool::new(false));
        GLOBAL_STOP_SIGNAL
            .set(new_signal.clone())
            .map_err(|_| anyhow!("Failed to set stop signal"))?;
        new_signal
    };

    // Reset stop signal on (re)start
    stop_signal.store(false, Ordering::Relaxed);

    // Copy callback for use in closures
    let callback_copy = callback;

    // Helper function to invoke callback
    let invoke_callback = move |status: &str, message: &str| {
        if let Some(callback_fn) = callback_copy {
            // Create combined message
            let combined = format!("{} - {}", status, message);
            let combined_cstr = match CString::new(combined) {
                Ok(s) => s,
                Err(_) => return,
            };

            // Call the C callback function
            unsafe {
                callback_fn(combined_cstr.as_ptr(), std::ptr::null_mut());
            }
        }
    };

    invoke_callback("STARTING", "Initializing background tasks...");

    // Collect device information dynamically in async context
    println!("🔧 Android: Collecting device information...");
    let (devices_info, device_count) = crate::util::system_info::collect_device_info()
        .await
        .map_err(|e| anyhow!("Failed to collect device info: {}", e))?;

    println!(
        "✅ Android: Device info collected - {} devices, total memory: {}GB",
        devices_info.num, devices_info.memtotal_gb
    );

    // Get the stored TCP connection from android_login module
    let tcp_stream =
        get_android_tcp_stream().ok_or_else(|| anyhow!("TCP connection not initialized"))?;

    // Clone device info for use in threads
    let device_info_for_heartbeat = devices_info.clone();
    let device_info_for_handler = devices_info.clone();

    // Spawn heartbeat task using native thread with full heartbeat logic
    let heartbeat_stream = tcp_stream.clone();
    let heartbeat_callback = callback;
    let heartbeat_stop_signal = stop_signal.clone();
    let heartbeat_handle = thread::spawn(move || {
        println!("🔧 Android: Heartbeat thread started");

        loop {
            // Check stop signal before sleeping
            if heartbeat_stop_signal.load(Ordering::Relaxed) {
                println!("🔧 Android: Heartbeat thread received stop signal");
                break;
            }

            println!("🔧 Android: Heartbeat loop - sleeping for 120 seconds...");

            println!("💓 Android: Woke up - collecting system info for heartbeat...");

            // Invoke callback for heartbeat
            if let Some(callback_fn) = heartbeat_callback {
                let heartbeat_msg = match CString::new("HEARTBEAT - Sending heartbeat to server") {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                unsafe {
                    callback_fn(heartbeat_msg.as_ptr(), std::ptr::null_mut());
                }
            }

            // Get real-time system usage information
            let (cpu_usage, memory_usage, disk_usage) = get_realtime_system_usage();

            // Get network usage information
            let (network_rx, network_tx) = get_network_usage();

            // Use dynamically collected device info (cloned from async context)
            let device_info = device_info_for_heartbeat.clone();

            println!(
                "💓 Android: Sending heartbeat - CPU: {}% MEM: {}% DISK: {}% NET: ↑{}B ↓{}B MEM_TOTAL: {}GB",
                cpu_usage, memory_usage, disk_usage, network_tx, network_rx, device_info.memtotal_gb
            );

            // Send heartbeat using independent TCP connection to avoid lock conflicts
            let server_addr = match ANDROID_SERVER_ADDR
                .get()
                .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
            {
                Some(addr) => addr,
                None => {
                    eprintln!("❌ Android: Server address not set");
                    continue;
                }
            };

            let control_port = match ANDROID_CONTROL_PORT
                .get()
                .and_then(|m| m.lock().ok().and_then(|g| *g))
            {
                Some(port) => port,
                None => {
                    eprintln!("❌ Android: Control port not set");
                    continue;
                }
            };

            let mut stream =
                match std::net::TcpStream::connect(format!("{}:{}", server_addr, control_port)) {
                    Ok(s) => {
                        println!("✅ Android: Connected to server for heartbeat");
                        s
                    }
                    Err(e) => {
                        eprintln!("❌ Android: Failed to connect for heartbeat: {}", e);
                        if let Some(callback_fn) = heartbeat_callback {
                            let error_msg =
                                match CString::new("ERROR - Failed to connect for heartbeat") {
                                    Ok(s) => s,
                                    Err(_) => continue,
                                };
                            unsafe {
                                callback_fn(error_msg.as_ptr(), std::ptr::null_mut());
                            }
                        }
                        continue;
                    }
                };

            // Create heartbeat command
            let client_id = ANDROID_CLIENT_ID
                .get()
                .and_then(|m| m.lock().ok().and_then(|g| *g))
                .unwrap_or([0u8; 16]);
            let heartbeat_cmd = CommandV1::Heartbeat {
                client_id,
                system_info: SystemInfo {
                    cpu_usage: cpu_usage as u8,
                    memory_usage: memory_usage as u8,
                    disk_usage: disk_usage as u8,
                    network_rx, // Real network RX bytes (in KB)
                    network_tx, // Real network TX bytes (in KB)
                },
                device_memtotal_gb: device_info.memtotal_gb.try_into().unwrap_or(0),
                device_total_tflops: device_info.total_tflops.into(),
                device_count: device_info.num as u16,
                devices_info: vec![device_info],
            };

            // Send heartbeat using common library function
            if let Err(e) = common::write_command_sync(&mut stream, &Command::V1(heartbeat_cmd)) {
                eprintln!("❌ Android: Failed to send heartbeat: {}", e);
                println!("🔧 Android: Continuing heartbeat loop despite send failure...");
                if let Some(callback_fn) = heartbeat_callback {
                    let error_msg =
                        match CString::new(format!("ERROR - Failed to send heartbeat: {}", e)) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                    unsafe {
                        callback_fn(error_msg.as_ptr(), std::ptr::null_mut());
                    }
                }
            } else {
                println!("✅ Android: Heartbeat sent successfully");
                {
                    let current_model_path = crate::MODEL_STATUS
                        .lock()
                        .ok()
                        .and_then(|s| s.current_model.clone())
                        .unwrap_or_else(|| "android".to_string());
                    let model_id = derive_model_id_from_path(&current_model_path);
                    let models = vec![Model {
                        id: model_id,
                        object: "model".to_string(),
                        created: 0,
                        owned_by: "android".to_string(),
                    }];
                    let model_status = CommandV1::ModelStatus {
                        client_id,
                        models,
                        auto_models_device: Vec::new(),
                    };
                    if let Err(e) =
                        common::write_command_sync(&mut stream, &Command::V1(model_status))
                    {
                        eprintln!("❌ Android: Failed to send model status: {}", e);
                    } else {
                        println!("✅ Android: Model status sent successfully");
                    }
                    std::io::stdout().flush().ok();
                }
                if let Some(callback_fn) = heartbeat_callback {
                    let success_msg = match CString::new("SUCCESS - Heartbeat sent successfully") {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    unsafe {
                        callback_fn(success_msg.as_ptr(), std::ptr::null_mut());
                    }
                }
            }

            // Close the connection after sending heartbeat
            drop(stream);
            println!("🔧 Android: Heartbeat connection closed, starting next iteration...");

            // Sleep with periodic stop signal checks
            for _ in 0..120 {
                // 120 seconds / 1 second intervals
                thread::sleep(Duration::from_secs(1));
                if heartbeat_stop_signal.load(Ordering::Relaxed) {
                    println!("🔧 Android: Heartbeat thread received stop signal during sleep");
                    break;
                }
            }

            // Check stop signal after sleep
            if heartbeat_stop_signal.load(Ordering::Relaxed) {
                println!("🔧 Android: Heartbeat thread received stop signal after sleep");
                break;
            }
        }
        println!("🔧 Android: Heartbeat thread stopped");
    });

    // Spawn integrated handler task using native thread
    let handler_stream = tcp_stream.clone();
    let handler_callback = callback;
    let handler_stop_signal = stop_signal.clone();
    let handler_handle = thread::spawn(move || -> Result<()> {
        println!("🔧 Android: Integrated handler thread started");
        std::io::stdout().flush().ok();

        // Invoke callback for handler start
        if let Some(callback_fn) = handler_callback {
            let start_msg = match CString::new("HANDLER_START - Handler thread started") {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            unsafe {
                callback_fn(start_msg.as_ptr(), std::ptr::null_mut());
            }
        }

        // Ensure blocking reads wake up periodically so stop signal can be observed
        // (read_command_sync uses read_exact and can otherwise block forever)
        {
            if let Ok(mut stream) = handler_stream.lock() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            }
        }

        loop {
            // Check stop signal before waiting for command
            if handler_stop_signal.load(Ordering::Relaxed) {
                println!("🔧 Android: Handler thread received stop signal");
                break;
            }

            // Try to get stream lock with timeout to avoid deadlock
            let stream_result = {
                let stream = handler_stream.try_lock();
                stream
            };

            let mut stream = match stream_result {
                Ok(s) => s,
                Err(_) => {
                    // Lock is contended, wait a bit and retry
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
            };

            // Read command using common library function
            match common::read_command_sync(&mut *stream) {
                Ok(command) => {
                    println!("🔧 Android: Received command: {:?}", command);
                    std::io::stdout().flush().ok();

                    // Invoke callback for received command
                    if let Some(callback_fn) = handler_callback {
                        let cmd_str = format!("COMMAND_RECEIVED - {:?}", command);
                        let cmd_msg = match CString::new(cmd_str) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        unsafe {
                            callback_fn(cmd_msg.as_ptr(), std::ptr::null_mut());
                        }
                    }

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
                                        let client_id = ANDROID_CLIENT_ID
                                            .get()
                                            .and_then(|m| m.lock().ok().and_then(|g| *g))
                                            .unwrap_or([0u8; 16]);
                                        let current_model_path = crate::MODEL_STATUS
                                            .lock()
                                            .ok()
                                            .and_then(|s| s.current_model.clone())
                                            .unwrap_or_else(|| "android".to_string());
                                        let model_id =
                                            derive_model_id_from_path(&current_model_path);
                                        println!("model_id: {}", model_id);
                                        let models = vec![Model {
                                            id: model_id,
                                            object: "model".to_string(),
                                            created: 0,
                                            owned_by: "android".to_string(),
                                        }];
                                        let model_status = CommandV1::ModelStatus {
                                            client_id,
                                            models,
                                            auto_models_device: Vec::new(),
                                        };
                                        let _ = common::write_command_sync(
                                            &mut *stream,
                                            &Command::V1(model_status),
                                        );
                                        if let Some(callback_fn) = handler_callback {
                                            let success_msg = match CString::new(
                                                "LOGIN_SUCCESS - Login successful",
                                            ) {
                                                Ok(s) => s,
                                                Err(_) => continue,
                                            };
                                            unsafe {
                                                callback_fn(
                                                    success_msg.as_ptr(),
                                                    std::ptr::null_mut(),
                                                );
                                            }
                                        }
                                        if !pods_model.is_empty() {
                                            println!(
                                                "🔧 Android: Received {} models",
                                                pods_model.len()
                                            );
                                            for pod_model in &pods_model {
                                                if let Some(model_name) = &pod_model.model_name {
                                                    println!("📦 Android: Model: {}", model_name);
                                                }
                                            }
                                        }
                                    } else {
                                        eprintln!("❌ Android: Login failed: {:?}", error);
                                        if let Some(callback_fn) = handler_callback {
                                            let error_str = format!("LOGIN_FAILED - {:?}", error);
                                            let error_msg = match CString::new(error_str) {
                                                Ok(s) => s,
                                                Err(_) => break,
                                            };
                                            unsafe {
                                                callback_fn(
                                                    error_msg.as_ptr(),
                                                    std::ptr::null_mut(),
                                                );
                                            }
                                        }
                                        break;
                                    }
                                }
                                CommandV1::PullModelResult { pods_model, error } => {
                                    if let Some(err) = error {
                                        eprintln!("❌ Android: Pull model failed: {}", err);
                                        if let Some(callback_fn) = handler_callback {
                                            let error_str = format!("PULL_MODEL_FAILED - {}", err);
                                            let error_msg = match CString::new(error_str) {
                                                Ok(s) => s,
                                                Err(_) => continue,
                                            };
                                            unsafe {
                                                callback_fn(
                                                    error_msg.as_ptr(),
                                                    std::ptr::null_mut(),
                                                );
                                            }
                                        }
                                    } else {
                                        println!("✅ Android: Pull model successful");
                                        if let Some(callback_fn) = handler_callback {
                                            let success_msg = match CString::new(
                                                "PULL_MODEL_SUCCESS - Pull model successful",
                                            ) {
                                                Ok(s) => s,
                                                Err(_) => continue,
                                            };
                                            unsafe {
                                                callback_fn(
                                                    success_msg.as_ptr(),
                                                    std::ptr::null_mut(),
                                                );
                                            }
                                        }
                                        if !pods_model.is_empty() {
                                            println!(
                                                "📦 Android: Received {} models",
                                                pods_model.len()
                                            );
                                            for pod_model in &pods_model {
                                                if let Some(model_name) = &pod_model.model_name {
                                                    println!("📦 Android: Model: {}", model_name);
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
                                    repeat_last_n: _,
                                    min_keep: _,
                                } => {
                                    println!("🔧 Android: Received inference task: {}", task_id);
                                    println!("📝 Android: Prompt: {}", prompt);
                                    println!("⚙️ Android: Parameters: max_tokens={}, temp={}, top_k={}, top_p={}", 
                                                             max_tokens, temperature, top_k, top_p);

                                    // Invoke callback for inference task start
                                    invoke_callback(
                                        "INFERENCE_START",
                                        &format!("Task: {}", task_id),
                                    );

                                    use crate::llama_context;
                                    use crate::{
                                        gpuf_start_generation_async, GLOBAL_CONTEXT_PTR,
                                        GLOBAL_INFERENCE_MUTEX,
                                    };
                                    use std::ffi::CString;
                                    use std::os::raw::c_void;
                                    let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);
                                    if context_ptr.is_null() {
                                        let err = "Model not loaded - please load a model first"
                                            .to_string();
                                        let result_command = CommandV1::InferenceResultChunk {
                                            task_id: task_id.clone(),
                                            seq: 0,
                                            delta: String::new(),
                                            done: true,
                                            error: Some(err.clone()),
                                            prompt_tokens: 0,
                                            completion_tokens: 0,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut *stream,
                                            &Command::V1(result_command),
                                        );
                                        invoke_callback(
                                            "INFERENCE_FAILED",
                                            &format!("Task: {} Error: {}", task_id, err),
                                        );
                                        continue;
                                    }

                                    {
                                        let mut active = ANDROID_ACTIVE_TASK_ID
                                            .get()
                                            .expect("ANDROID_ACTIVE_TASK_ID is set")
                                            .lock()
                                            .unwrap();
                                        *active = Some(task_id.clone());
                                    }

                                    let writer_stream = match stream.try_clone() {
                                        Ok(s) => s,
                                        Err(e) => {
                                            let err = format!("Failed to clone TCP stream: {}", e);
                                            let result_command = CommandV1::InferenceResultChunk {
                                                task_id: task_id.clone(),
                                                seq: 0,
                                                delta: String::new(),
                                                done: true,
                                                error: Some(err.clone()),
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            let _ = common::write_command_sync(
                                                &mut *stream,
                                                &Command::V1(result_command),
                                            );
                                            invoke_callback(
                                                "INFERENCE_FAILED",
                                                &format!("Task: {} Error: {}", task_id, err),
                                            );
                                            continue;
                                        }
                                    };

                                    let task_id_for_thread = task_id.clone();
                                    let prompt_for_thread = prompt.clone();
                                    let context_ptr_usize = context_ptr as usize;
                                    std::thread::spawn(move || {
                                        let context_ptr = context_ptr_usize as *mut llama_context;
                                        #[repr(C)]
                                        struct TokenCallbackState {
                                            stream: std::net::TcpStream,
                                            task_id: String,
                                            seq: u32,
                                            buf: String,
                                            max_bytes: usize,
                                            prompt_tokens: u32,
                                            completion_tokens: u32,
                                            suppress: bool,
                                        }

                                        extern "C" fn on_token(
                                            token: *const std::os::raw::c_char,
                                            user_data: *mut c_void,
                                        ) {
                                            if token.is_null() || user_data.is_null() {
                                                return;
                                            }

                                            let state = unsafe {
                                                &mut *(user_data as *mut TokenCallbackState)
                                            };
                                            let token_str =
                                                unsafe { std::ffi::CStr::from_ptr(token).to_str() };
                                            let Ok(token_str) = token_str else {
                                                return;
                                            };

                                            if state.suppress {
                                                if token_str.contains("assistantfinal")
                                                    || token_str.contains("final")
                                                {
                                                    state.suppress = false;
                                                }
                                                return;
                                            }

                                            if token_should_be_suppressed(token_str)
                                                && state.completion_tokens < 128
                                            {
                                                state.suppress = true;
                                                state.buf.clear();
                                                return;
                                            }

                                            state.buf.push_str(token_str);
                                            state.completion_tokens =
                                                state.completion_tokens.saturating_add(1);
                                            if state.buf.len() < state.max_bytes {
                                                return;
                                            }

                                            let delta = std::mem::take(&mut state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: state.task_id.clone(),
                                                seq: state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: state.prompt_tokens,
                                                completion_tokens: state.completion_tokens,
                                            };
                                            state.seq = state.seq.wrapping_add(1);

                                            let _ = common::write_command_sync(
                                                &mut state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();
                                        let start_time = std::time::Instant::now();
                                        let prompt_cstr = match CString::new(prompt_for_thread) {
                                            Ok(s) => s,
                                            Err(e) => {
                                                let err = format!("Invalid prompt: {}", e);
                                                let mut s = writer_stream;
                                                let result_command =
                                                    CommandV1::InferenceResultChunk {
                                                        task_id: task_id_for_thread.clone(),
                                                        seq: 0,
                                                        delta: String::new(),
                                                        done: true,
                                                        error: Some(err.clone()),
                                                        prompt_tokens: 0,
                                                        completion_tokens: 0,
                                                    };
                                                let _ = common::write_command_sync(
                                                    &mut s,
                                                    &Command::V1(result_command),
                                                );
                                                invoke_callback(
                                                    "INFERENCE_FAILED",
                                                    &format!(
                                                        "Task: {} Error: {}",
                                                        task_id_for_thread, err
                                                    ),
                                                );
                                                return;
                                            }
                                        };

                                        let prompt_tokens: u32 =
                                            count_prompt_tokens(context_ptr, prompt_cstr.as_ptr());

                                        let mut cb_state = TokenCallbackState {
                                            stream: writer_stream,
                                            task_id: task_id_for_thread.clone(),
                                            seq: 0,
                                            buf: String::new(),
                                            max_bytes: 8,
                                            prompt_tokens,
                                            completion_tokens: 0,
                                            suppress: false,
                                        };

                                        let completion_tokens_i32 = unsafe {
                                            gpuf_start_generation_async(
                                                context_ptr,
                                                prompt_cstr.as_ptr(),
                                                max_tokens as i32,
                                                temperature,
                                                top_k as i32,
                                                top_p,
                                                repeat_penalty,
                                                Some(on_token),
                                                (&mut cb_state as *mut TokenCallbackState)
                                                    as *mut c_void,
                                            )
                                        };

                                        if completion_tokens_i32 > 0 {
                                            cb_state.completion_tokens = completion_tokens_i32 as u32;
                                        }

                                        if !cb_state.buf.is_empty() {
                                            let delta = std::mem::take(&mut cb_state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: task_id_for_thread.clone(),
                                                seq: cb_state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: cb_state.prompt_tokens,
                                                completion_tokens: cb_state.completion_tokens,
                                            };
                                            cb_state.seq = cb_state.seq.wrapping_add(1);
                                            let _ = common::write_command_sync(
                                                &mut cb_state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let done_chunk = CommandV1::InferenceResultChunk {
                                            task_id: task_id_for_thread.clone(),
                                            seq: cb_state.seq,
                                            delta: String::new(),
                                            done: true,
                                            error: None,
                                            prompt_tokens: cb_state.prompt_tokens,
                                            completion_tokens: cb_state.completion_tokens,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut cb_state.stream,
                                            &Command::V1(done_chunk),
                                        );

                                        {
                                            let mut active = ANDROID_ACTIVE_TASK_ID
                                                .get()
                                                .expect("ANDROID_ACTIVE_TASK_ID is set")
                                                .lock()
                                                .unwrap();
                                            if active.as_deref()
                                                == Some(task_id_for_thread.as_str())
                                            {
                                                *active = None;
                                            }
                                        }

                                        let execution_time =
                                            start_time.elapsed().as_millis() as u64;
                                        invoke_callback(
                                            "INFERENCE_SUCCESS",
                                            &format!(
                                                "Task: {} in {}ms",
                                                task_id_for_thread, execution_time
                                            ),
                                        );
                                        invoke_callback(
                                            "SUCCESS",
                                            "Inference result sent successfully",
                                        );
                                    });
                                }

                                CommandV1::ChatInferenceTask {
                                    task_id,
                                    model: _,
                                    messages,
                                    max_tokens,
                                    temperature,
                                    top_k,
                                    top_p,
                                    repeat_penalty,
                                    repeat_last_n: _,
                                    min_keep: _,
                                } => {
                                    println!(
                                        "🔧 Android: Received chat inference task: {}",
                                        task_id
                                    );

                                    invoke_callback(
                                        "INFERENCE_START",
                                        &format!("Task: {}", task_id),
                                    );

                                    use crate::llama_context;
                                    use crate::{
                                        gpuf_start_generation_async, GLOBAL_CONTEXT_PTR,
                                        GLOBAL_INFERENCE_MUTEX,
                                    };
                                    use std::ffi::CString;
                                    use std::os::raw::c_void;

                                    let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);
                                    if context_ptr.is_null() {
                                        let err =
                                            "Model not loaded - please load a model first".to_string();
                                        let result_command = CommandV1::InferenceResultChunk {
                                            task_id: task_id.clone(),
                                            seq: 0,
                                            delta: String::new(),
                                            done: true,
                                            error: Some(err.clone()),
                                            prompt_tokens: 0,
                                            completion_tokens: 0,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut *stream,
                                            &Command::V1(result_command),
                                        );
                                        invoke_callback(
                                            "INFERENCE_FAILED",
                                            &format!("Task: {} Error: {}", task_id, err),
                                        );
                                        continue;
                                    }

                                    let prompt = build_chat_prompt_with_gguf_template(
                                        context_ptr,
                                        &messages,
                                    )
                                    .unwrap_or_else(|| build_chat_prompt(&messages));

                                    {
                                        let mut active = ANDROID_ACTIVE_TASK_ID
                                            .get()
                                            .expect("ANDROID_ACTIVE_TASK_ID is set")
                                            .lock()
                                            .unwrap();
                                        *active = Some(task_id.clone());
                                    }

                                    let writer_stream = match stream.try_clone() {
                                        Ok(s) => s,
                                        Err(e) => {
                                            let err = format!("Failed to clone TCP stream: {}", e);
                                            let result_command = CommandV1::InferenceResultChunk {
                                                task_id: task_id.clone(),
                                                seq: 0,
                                                delta: String::new(),
                                                done: true,
                                                error: Some(err.clone()),
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            let _ = common::write_command_sync(
                                                &mut *stream,
                                                &Command::V1(result_command),
                                            );
                                            invoke_callback(
                                                "INFERENCE_FAILED",
                                                &format!("Task: {} Error: {}", task_id, err),
                                            );
                                            continue;
                                        }
                                    };

                                    let task_id_for_thread = task_id.clone();
                                    let prompt_for_thread = prompt.clone();
                                    let context_ptr_usize = context_ptr as usize;
                                    std::thread::spawn(move || {
                                        let context_ptr = context_ptr_usize as *mut llama_context;

                                        #[repr(C)]
                                        struct TokenCallbackState {
                                            stream: std::net::TcpStream,
                                            task_id: String,
                                            seq: u32,
                                            buf: String,
                                            max_bytes: usize,
                                            prompt_tokens: u32,
                                            completion_tokens: u32,
                                            suppress: bool,
                                        }

                                        extern "C" fn on_token(
                                            token: *const std::os::raw::c_char,
                                            user_data: *mut c_void,
                                        ) {
                                            if token.is_null() || user_data.is_null() {
                                                return;
                                            }

                                            let state = unsafe {
                                                &mut *(user_data as *mut TokenCallbackState)
                                            };
                                            let token_str =
                                                unsafe { std::ffi::CStr::from_ptr(token).to_str() };
                                            let Ok(token_str) = token_str else {
                                                return;
                                            };

                                            if state.suppress {
                                                if token_str.contains("assistantfinal")
                                                    || token_str.contains("final")
                                                {
                                                    state.suppress = false;
                                                }
                                                return;
                                            }

                                            if token_should_be_suppressed(token_str)
                                                && state.completion_tokens < 128
                                            {
                                                state.suppress = true;
                                                state.buf.clear();
                                                return;
                                            }

                                            state.buf.push_str(token_str);
                                            state.completion_tokens =
                                                state.completion_tokens.saturating_add(1);
                                            if state.buf.len() < state.max_bytes {
                                                return;
                                            }

                                            let delta = std::mem::take(&mut state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: state.task_id.clone(),
                                                seq: state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: state.prompt_tokens,
                                                completion_tokens: state.completion_tokens,
                                            };
                                            state.seq = state.seq.wrapping_add(1);

                                            let _ = common::write_command_sync(
                                                &mut state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();
                                        let start_time = std::time::Instant::now();
                                        let prompt_cstr = match CString::new(prompt_for_thread) {
                                            Ok(s) => s,
                                            Err(e) => {
                                                let err = format!("Invalid prompt: {}", e);
                                                let mut s = writer_stream;
                                                let result_command =
                                                    CommandV1::InferenceResultChunk {
                                                        task_id: task_id_for_thread.clone(),
                                                        seq: 0,
                                                        delta: String::new(),
                                                        done: true,
                                                        error: Some(err.clone()),
                                                        prompt_tokens: 0,
                                                        completion_tokens: 0,
                                                    };
                                                let _ = common::write_command_sync(
                                                    &mut s,
                                                    &Command::V1(result_command),
                                                );
                                                invoke_callback(
                                                    "INFERENCE_FAILED",
                                                    &format!(
                                                        "Task: {} Error: {}",
                                                        task_id_for_thread, err
                                                    ),
                                                );
                                                return;
                                            }
                                        };

                                        let prompt_tokens: u32 =
                                            count_prompt_tokens(context_ptr, prompt_cstr.as_ptr());

                                        let mut cb_state = TokenCallbackState {
                                            stream: writer_stream,
                                            task_id: task_id_for_thread.clone(),
                                            seq: 0,
                                            buf: String::new(),
                                            max_bytes: 8,
                                            prompt_tokens,
                                            completion_tokens: 0,
                                            suppress: false,
                                        };

                                        let completion_tokens = unsafe {
                                            gpuf_start_generation_async(
                                                context_ptr,
                                                prompt_cstr.as_ptr(),
                                                max_tokens as i32,
                                                temperature,
                                                top_k as i32,
                                                top_p,
                                                repeat_penalty,
                                                Some(on_token),
                                                (&mut cb_state as *mut TokenCallbackState)
                                                    as *mut c_void,
                                            )
                                        };

                                        cb_state.completion_tokens = completion_tokens as u32;

                                        if !cb_state.buf.is_empty() {
                                            let delta = std::mem::take(&mut cb_state.buf);
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id: task_id_for_thread.clone(),
                                                seq: cb_state.seq,
                                                delta,
                                                done: false,
                                                error: None,
                                                prompt_tokens: cb_state.prompt_tokens,
                                                completion_tokens: cb_state.completion_tokens,
                                            };
                                            cb_state.seq = cb_state.seq.wrapping_add(1);
                                            let _ = common::write_command_sync(
                                                &mut cb_state.stream,
                                                &Command::V1(chunk),
                                            );
                                        }

                                        let done_chunk = CommandV1::InferenceResultChunk {
                                            task_id: task_id_for_thread.clone(),
                                            seq: cb_state.seq,
                                            delta: String::new(),
                                            done: true,
                                            error: None,
                                            prompt_tokens: cb_state.prompt_tokens,
                                            completion_tokens: cb_state.completion_tokens,
                                        };
                                        let _ = common::write_command_sync(
                                            &mut cb_state.stream,
                                            &Command::V1(done_chunk),
                                        );

                                        {
                                            let mut active = ANDROID_ACTIVE_TASK_ID
                                                .get()
                                                .expect("ANDROID_ACTIVE_TASK_ID is set")
                                                .lock()
                                                .unwrap();
                                            if active.as_deref()
                                                == Some(task_id_for_thread.as_str())
                                            {
                                                *active = None;
                                            }
                                        }

                                        let execution_time =
                                            start_time.elapsed().as_millis() as u64;
                                        invoke_callback(
                                            "INFERENCE_SUCCESS",
                                            &format!(
                                                "Task: {} in {}ms",
                                                task_id_for_thread, execution_time
                                            ),
                                        );
                                    });
                                }

                                CommandV1::CancelInference { task_id } => {
                                    let should_cancel = {
                                        let active_lock = ANDROID_ACTIVE_TASK_ID
                                            .get()
                                            .expect("ANDROID_ACTIVE_TASK_ID is set")
                                            .lock()
                                            .unwrap();
                                        active_lock.as_deref() == Some(task_id.as_str())
                                    };

                                    if should_cancel {
                                        use crate::{gpuf_stop_generation, GLOBAL_CONTEXT_PTR};
                                        let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);
                                        if !context_ptr.is_null() {
                                            unsafe { gpuf_stop_generation(context_ptr) };
                                        }
                                    }
                                }
                                _ => {
                                    println!("⚠️ Android: Received unhandled command type");
                                    invoke_callback("WARNING", "Received unhandled command type");
                                }
                            }
                        }
                        _ => {
                            println!("⚠️ Android: Received non-V1 command");
                            invoke_callback("WARNING", "Received non-V1 command");
                        }
                    }
                }
                Err(e) => {
                    if let Some(ioe) = e.downcast_ref::<std::io::Error>() {
                        if matches!(
                            ioe.kind(),
                            std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::Interrupted
                        ) {
                            drop(stream);
                            continue;
                        }
                    }
                    eprintln!("❌ Android: Failed to read command: {}", e);
                    invoke_callback("ERROR", &format!("Failed to read command: {}", e));
                    break;
                }
            }

            // Release lock before next iteration
            drop(stream);
        }

        println!("🔧 Android: Integrated handler thread stopped");
        if let Some(callback_fn) = handler_callback {
            let stop_msg = match CString::new("HANDLER_STOP - Handler thread stopped") {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            unsafe {
                callback_fn(stop_msg.as_ptr(), std::ptr::null_mut());
            }
        }
        Ok(())
    });

    // Store thread handles for cleanup (support multiple start/stop cycles)
    let handles = GLOBAL_WORKER_HANDLES.get_or_init(|| Mutex::new(None));
    {
        let mut guard = handles.lock().unwrap();
        *guard = Some((heartbeat_handle, handler_handle));
    }

    info!("✅ Android: Background tasks with callback started successfully");
    invoke_callback("SUCCESS", "Background tasks started successfully");

    Ok(())
}

/// Stop global worker and cleanup
#[cfg(target_os = "android")]
pub async fn stop_global_worker() {
    // Set stop signal to notify background threads
    if let Some(stop_signal) = GLOBAL_STOP_SIGNAL.get() {
        stop_signal.store(true, Ordering::Relaxed);
        tracing::info!("Stop signal sent to background threads");
    }

    // Wait for background tasks to finish
    let handles_opt = GLOBAL_WORKER_HANDLES
        .get()
        .and_then(|m| m.lock().ok().and_then(|mut g| g.take()));

    if let Some((heartbeat_handle, handler_handle)) = handles_opt {
        tracing::info!("Waiting for heartbeat thread to finish...");
        match heartbeat_handle.join() {
            Ok(()) => tracing::info!("Heartbeat thread finished successfully"),
            Err(e) => tracing::error!("Heartbeat thread panicked: {:?}", e),
        }

        tracing::info!("Waiting for handler thread to finish...");
        match handler_handle.join() {
            Ok(Ok(())) => tracing::info!("Handler thread finished successfully"),
            Ok(Err(e)) => tracing::error!("Handler thread returned error: {:?}", e),
            Err(e) => tracing::error!("Handler thread panicked: {:?}", e),
        }

        tracing::info!("All background threads stopped");
    }

    if let Some(m) = ANDROID_TCP_STREAM.get() {
        if let Ok(mut guard) = m.lock() {
            if let Some(stream) = guard.take() {
                if let Ok(stream) = stream.lock() {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                }
            }
        }
    }

    if let Some(m) = ANDROID_SERVER_ADDR.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }

    if let Some(m) = ANDROID_CONTROL_PORT.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }

    if let Some(m) = ANDROID_CLIENT_ID.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
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