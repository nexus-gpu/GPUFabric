use crate::util::mobile_control_stream::{
    connect_mobile_control_stream, MobileControlStream, MobileControlTlsConfig,
};
use anyhow::{anyhow, Result};
use common::{
    Command, CommandV1, DevicesInfo, EngineType as CommonEngineType, Model, OsType, SystemInfo,
};
use std::ffi::{c_char, c_void};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const CURRENT_VERSION: u32 = 1;

fn derive_model_id_from_path(model_path: &str) -> String {
    let lower = model_path.to_ascii_lowercase();
    if lower.contains("llama-3") || lower.contains("llama3") {
        return "llama3".to_string();
    }
    if lower.contains("qwen") {
        return "qwen".to_string();
    }
    if lower.contains("mistral") {
        return "mistral".to_string();
    }
    "llama".to_string()
}

fn emit_callback(callback: Option<extern "C" fn(*const c_char, *mut c_void)>, msg: &str) {
    let Some(cb) = callback else { return };
    let Ok(cmsg) = std::ffi::CString::new(msg) else {
        return;
    };
    cb(cmsg.as_ptr(), std::ptr::null_mut());
}

static WORKER_TCP_STREAM: OnceLock<Mutex<Option<Arc<Mutex<MobileControlStream>>>>> =
    OnceLock::new();
static WORKER_SERVER_ADDR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static WORKER_CONTROL_PORT: OnceLock<Mutex<Option<u16>>> = OnceLock::new();
static WORKER_CONTROL_TLS: OnceLock<Mutex<MobileControlTlsConfig>> = OnceLock::new();
static WORKER_CLIENT_ID: OnceLock<Mutex<Option<[u8; 16]>>> = OnceLock::new();
static WORKER_STOP_SIGNAL: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static WORKER_CANCELLED_TASK: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn os_type() -> OsType {
    #[cfg(target_os = "ios")]
    {
        return OsType::IOS;
    }
    #[cfg(target_os = "android")]
    {
        return OsType::ANDROID;
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        return OsType::NONE;
    }
}

pub fn get_tcp_stream() -> Option<Arc<Mutex<MobileControlStream>>> {
    WORKER_TCP_STREAM
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
}

fn clear_tcp_stream() {
    if let Some(slot) = WORKER_TCP_STREAM.get() {
        if let Ok(mut guard) = slot.lock() {
            *guard = None;
        }
    }
}

fn get_worker_control_tls_config() -> MobileControlTlsConfig {
    WORKER_CONTROL_TLS
        .get()
        .and_then(|m| m.lock().ok().map(|g| g.clone()))
        .unwrap_or_else(MobileControlTlsConfig::plaintext)
}

pub async fn perform_login(
    server_addr: &str,
    control_port: u16,
    client_id_hex: &str,
    auto_models: bool,
) -> Result<()> {
    perform_login_with_tls(
        server_addr,
        control_port,
        client_id_hex,
        auto_models,
        MobileControlTlsConfig::plaintext(),
    )
    .await
}

pub async fn perform_login_with_tls(
    server_addr: &str,
    control_port: u16,
    client_id_hex: &str,
    auto_models: bool,
    tls_config: MobileControlTlsConfig,
) -> Result<()> {
    // If there is an existing worker running, stop its background tasks and close its TCP stream.
    // Otherwise, old heartbeat threads can keep sending on the old connection, making the server
    // appear to still use the previous client_id.
    if let Some(stop) = WORKER_STOP_SIGNAL.get() {
        stop.store(true, Ordering::Relaxed);
    }
    if let Some(slot) = WORKER_TCP_STREAM.get() {
        if let Ok(mut guard) = slot.lock() {
            if let Some(existing) = guard.take() {
                if let Ok(s) = existing.lock() {
                    let _ = s.shutdown(std::net::Shutdown::Both);
                }
            }
        }
    }

    let mut stream = connect_mobile_control_stream(server_addr, control_port, &tls_config)?;

    let system_info = SystemInfo::default();

    // Use a fixed DevicesInfo to avoid heavy platform-specific probes.
    let mut fixed_devices_info = DevicesInfo::default();
    fixed_devices_info.num = 1;
    fixed_devices_info.pod_id = 0;
    fixed_devices_info.os_type = os_type();
    fixed_devices_info.engine_type = CommonEngineType::Llama;
    // Default vendor/device ids to avoid server-side assumptions.
    fixed_devices_info.vendor_id = 0x41;
    fixed_devices_info.device_id = 0x1000;

    let decoded = hex::decode(client_id_hex)
        .map_err(|e| anyhow!("Invalid client_id hex (expected 32 hex chars): {e}"))?;
    let client_id: [u8; 16] = decoded
        .try_into()
        .map_err(|_| anyhow!("Invalid client_id length (expected 16 bytes / 32 hex chars)"))?;

    let login_cmd = CommandV1::Login {
        version: CURRENT_VERSION,
        auto_models,
        os_type: os_type(),
        client_id,
        system_info,
        device_memtotal_gb: 0,
        device_total_tflops: 0,
        devices_info: vec![fixed_devices_info],
    };

    common::write_command_sync(&mut stream, &Command::V1(login_cmd))
        .map_err(|e| anyhow!("Failed to send login command: {}", e))?;

    let stream_arc = Arc::new(Mutex::new(stream));
    {
        let slot = WORKER_TCP_STREAM.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(stream_arc);
    }
    {
        let slot = WORKER_SERVER_ADDR.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(server_addr.to_string());
    }
    {
        let slot = WORKER_CONTROL_PORT.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(control_port);
    }
    {
        let slot =
            WORKER_CONTROL_TLS.get_or_init(|| Mutex::new(MobileControlTlsConfig::plaintext()));
        let mut guard = slot.lock().unwrap();
        *guard = tls_config;
    }
    {
        let slot = WORKER_CLIENT_ID.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(client_id);
    }

    Ok(())
}

pub async fn start_worker_tasks_with_callback_ptr(
    callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
) -> Result<()> {
    let tcp_stream = get_tcp_stream().ok_or_else(|| anyhow!("TCP connection not initialized"))?;

    let stop_signal = if let Some(existing) = WORKER_STOP_SIGNAL.get() {
        existing.clone()
    } else {
        let new_signal = Arc::new(AtomicBool::new(false));
        let _ = WORKER_STOP_SIGNAL.set(new_signal.clone());
        new_signal
    };
    stop_signal.store(false, Ordering::Relaxed);

    let heartbeat_stop = stop_signal.clone();
    let heartbeat_callback = callback;
    let heartbeat_stream = tcp_stream.clone();
    std::thread::spawn(move || {
        loop {
            if heartbeat_stop.load(Ordering::Relaxed) {
                break;
            }

            let client_id = WORKER_CLIENT_ID
                .get()
                .and_then(|m| m.lock().ok().and_then(|g| *g))
                .unwrap_or([0u8; 16]);

            let system_info = SystemInfo::default();

            let mut fixed_devices_info = DevicesInfo::default();
            fixed_devices_info.num = 1;
            fixed_devices_info.pod_id = 0;
            fixed_devices_info.os_type = os_type();
            fixed_devices_info.engine_type = CommonEngineType::Llama;
            fixed_devices_info.vendor_id = 0x41;
            fixed_devices_info.device_id = 0x1000;

            let hb = CommandV1::Heartbeat {
                client_id,
                system_info,
                device_count: 1,
                device_memtotal_gb: 0,
                device_total_tflops: 0,
                devices_info: vec![fixed_devices_info],
            };

            let send_result = (|| {
                let mut stream = heartbeat_stream
                    .lock()
                    .map_err(|_| anyhow!("Heartbeat: stream mutex poisoned"))?;
                common::write_command_sync(&mut *stream, &Command::V1(hb))
                    .map_err(|e| anyhow!("Heartbeat: write_command_sync failed: {e}"))?;
                stream
                    .flush()
                    .map_err(|e| anyhow!("Heartbeat: flush failed: {e}"))?;
                Ok::<(), anyhow::Error>(())
            })();

            if let Some(cb) = heartbeat_callback {
                let msg = match send_result {
                    Ok(_) => "HEARTBEAT - Sent".to_string(),
                    Err(_) => "HEARTBEAT - Send failed".to_string(),
                };
                if let Ok(cmsg) = std::ffi::CString::new(msg) {
                    cb(cmsg.as_ptr(), std::ptr::null_mut());
                }
            }

            // Sleep 120s, but check stop signal every 1s so stop is responsive.
            for _ in 0..120 {
                if heartbeat_stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });

    let handler_stop = stop_signal.clone();
    let handler_callback = callback;
    let server_addr = WORKER_SERVER_ADDR
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
        .ok_or_else(|| {
            emit_callback(
                handler_callback,
                "FATAL: No server address configured; call start_remote_worker first",
            );
            anyhow!("Server address not configured; call start_remote_worker first")
        })?;
    let control_port = WORKER_CONTROL_PORT
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| *g))
        .ok_or_else(|| {
            emit_callback(
                handler_callback,
                "FATAL: No control port configured; call start_remote_worker first",
            );
            anyhow!("Control port not configured; call start_remote_worker first")
        })?;
    let tls_config = get_worker_control_tls_config();

    std::thread::spawn(move || {
        let mut consecutive_failures = 0u32;
        const MAX_CONSECUTIVE_FAILURES: u32 = 5;
        const RECONNECT_DELAY_MS: u64 = 5000; // 5 seconds

        loop {
            if handler_stop.load(Ordering::Relaxed) {
                break;
            }

            // Try to get a working stream
            let stream_arc = match get_tcp_stream() {
                Some(stream) => {
                    consecutive_failures = 0;
                    stream
                }
                None => {
                    consecutive_failures += 1;
                    emit_callback(
                        handler_callback,
                        &format!(
                            "CONNECTION_LOST - No control stream, attempting reconnect {}/{}",
                            consecutive_failures, MAX_CONSECUTIVE_FAILURES
                        ),
                    );

                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        emit_callback(
                            handler_callback,
                            "RECONNECT_FAILED - Max retries reached, stopping worker",
                        );
                        break;
                    }

                    match connect_mobile_control_stream(&server_addr, control_port, &tls_config) {
                        Ok(new_stream) => {
                            emit_callback(
                                handler_callback,
                                &format!(
                                    "RECONNECT_SUCCESS - Connected to configured endpoint (port={}, tls={})",
                                    control_port, tls_config.enabled
                                ),
                            );
                            let stream_arc = Arc::new(Mutex::new(new_stream));
                            if let Some(global_stream) = WORKER_TCP_STREAM.get() {
                                if let Ok(mut guard) = global_stream.lock() {
                                    *guard = Some(stream_arc.clone());
                                }
                            }
                            consecutive_failures = 0;
                            stream_arc
                        }
                        Err(e) => {
                            emit_callback(
                                handler_callback,
                                &format!(
                                    "RECONNECT_FAILED - Error redacted ({} bytes), retrying in {}s",
                                    e.to_string().len(),
                                    RECONNECT_DELAY_MS / 1000
                                ),
                            );
                            std::thread::sleep(std::time::Duration::from_millis(
                                RECONNECT_DELAY_MS,
                            ));
                            continue;
                        }
                    }
                }
            };

            let mut stream = match stream_arc.lock() {
                Ok(stream) => stream,
                Err(_) => {
                    consecutive_failures += 1;
                    emit_callback(
                        handler_callback,
                        &format!(
                            "STREAM_ERROR - Control stream mutex poisoned, failures={}",
                            consecutive_failures
                        ),
                    );
                    clear_tcp_stream();
                    std::thread::sleep(std::time::Duration::from_millis(RECONNECT_DELAY_MS));
                    continue;
                }
            };

            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .ok();

            // Process commands with this stream
            let mut stream_valid = true;
            while stream_valid && !handler_stop.load(Ordering::Relaxed) {
                let cmd = match common::read_command_sync(&mut *stream) {
                    Ok(c) => c,
                    Err(e) => {
                        if let Some(ioe) = e.downcast_ref::<std::io::Error>() {
                            if matches!(
                                ioe.kind(),
                                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                            ) {
                                continue;
                            }
                        }
                        emit_callback(
                            handler_callback,
                            &format!("READ_ERROR - Connection lost: {}", e),
                        );
                        clear_tcp_stream();
                        stream_valid = false;
                        break;
                    }
                };

                let Command::V1(v1) = cmd else {
                    continue;
                };

                match v1 {
                    CommandV1::LoginResult {
                        success,
                        pods_model: _,
                        error,
                    } => {
                        if !success {
                            let err = error.unwrap_or_else(|| "unknown".to_string());
                            emit_callback(
                                handler_callback,
                                &format!(
                                    "LOGIN_FAILED - server message redacted ({} bytes)",
                                    err.len()
                                ),
                            );
                            clear_tcp_stream();
                            stream_valid = false;
                            break;
                        }

                        emit_callback(handler_callback, "LOGIN_SUCCESS");

                        let client_id = WORKER_CLIENT_ID
                            .get()
                            .and_then(|m| m.lock().ok().and_then(|g| *g))
                            .unwrap_or([0u8; 16]);
                        let current_model_path = crate::MODEL_STATUS
                            .lock()
                            .ok()
                            .and_then(|s| s.current_model.clone())
                            .unwrap_or_else(|| "ios".to_string());
                        let model_id = derive_model_id_from_path(&current_model_path);

                        let models = vec![Model {
                            id: model_id,
                            object: "model".to_string(),
                            created: 0,
                            owned_by: "ios".to_string(),
                        }];

                        let model_status = CommandV1::ModelStatus {
                            client_id,
                            models,
                            auto_models_device: Vec::new(),
                        };

                        let _ =
                            common::write_command_sync(&mut *stream, &Command::V1(model_status));
                        emit_callback(handler_callback, "MODEL_STATUS_SENT");
                    }
                    CommandV1::CancelInference { task_id } => {
                        emit_callback(handler_callback, &format!("CANCEL_TASK - {task_id}"));
                        let slot = WORKER_CANCELLED_TASK.get_or_init(|| Mutex::new(None));
                        if let Ok(mut guard) = slot.lock() {
                            *guard = Some(task_id);
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
                        ..
                    } => {
                        emit_callback(handler_callback, &format!("INFERENCE_TASK - {task_id}"));
                        let effective_max_tokens = std::cmp::min(max_tokens, 512);
                        if effective_max_tokens != max_tokens {
                            emit_callback(
                            handler_callback,
                            &format!(
                                "INFERENCE_PARAMS - {task_id} max_tokens={max_tokens} clamped_to={effective_max_tokens}"
                            ),
                        );
                        }
                        emit_callback(handler_callback, &format!("INFERENCE_START - {task_id}"));
                        // Execute inference synchronously and send back chunks.
                        if let Err(e) = handle_inference_task(
                            &mut *stream,
                            task_id.clone(),
                            &prompt,
                            effective_max_tokens,
                            temperature,
                            std::cmp::min(top_k, i32::MAX as u32) as i32,
                            top_p,
                            repeat_penalty,
                        ) {
                            emit_callback(
                                handler_callback,
                                &format!("INFERENCE_FAILED - {task_id} - {e}"),
                            );
                        } else {
                            emit_callback(handler_callback, &format!("INFERENCE_DONE - {task_id}"));
                        }
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
                        ..
                    } => {
                        emit_callback(
                            handler_callback,
                            &format!("CHAT_INFERENCE_TASK - {task_id}"),
                        );
                        let effective_max_tokens = std::cmp::min(max_tokens, 512);
                        if effective_max_tokens != max_tokens {
                            emit_callback(
                            handler_callback,
                            &format!(
                                "INFERENCE_PARAMS - {task_id} max_tokens={max_tokens} clamped_to={effective_max_tokens}"
                            ),
                        );
                        }
                        emit_callback(handler_callback, &format!("INFERENCE_START - {task_id}"));

                        let prompt = build_chat_prompt_with_template(&messages);

                        if let Err(e) = handle_inference_task(
                            &mut *stream,
                            task_id.clone(),
                            &prompt,
                            effective_max_tokens,
                            temperature,
                            std::cmp::min(top_k, i32::MAX as u32) as i32,
                            top_p,
                            repeat_penalty,
                        ) {
                            emit_callback(
                                handler_callback,
                                &format!("INFERENCE_FAILED - {task_id} - {e}"),
                            );
                        } else {
                            emit_callback(handler_callback, &format!("INFERENCE_DONE - {task_id}"));
                        }
                    }
                    _ => {}
                }

                // If we reach here, the stream is still valid, continue processing commands
            }

            // If we exited the inner loop, the stream is invalid, go back to reconnection logic
            emit_callback(
                handler_callback,
                "Stream invalidated, attempting reconnection...",
            );
        }
    });

    Ok(())
}

fn build_chat_prompt_with_template(messages: &[common::ChatMessage]) -> String {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        use crate::llama_chat_message;

        // Try to use model's built-in chat template first
        let model_ptr = crate::GLOBAL_MODEL_PTR.load(std::sync::atomic::Ordering::SeqCst);
        if !model_ptr.is_null() {
            // Convert messages to C-style array
            let mut c_messages: Vec<llama_chat_message> = Vec::with_capacity(messages.len());
            let mut c_roles: Vec<std::ffi::CString> = Vec::with_capacity(messages.len());
            let mut c_contents: Vec<std::ffi::CString> = Vec::with_capacity(messages.len());

            for msg in messages {
                c_roles.push(std::ffi::CString::new(msg.role.clone()).unwrap_or_default());
                c_contents.push(std::ffi::CString::new(msg.content.clone()).unwrap_or_default());
                c_messages.push(llama_chat_message {
                    role: c_roles.last().unwrap().as_ptr(),
                    content: c_contents.last().unwrap().as_ptr(),
                });
            }

            // Try to apply model's built-in template (tmpl=nullptr).
            // SAFETY: `c_messages` points to role/content CStrings owned by
            // `c_roles`/`c_contents`, all of which live until the call returns.
            // `buffer` is a valid writable allocation of `buffer.len()` bytes.
            let mut buffer = vec![0u8; 8192]; // 8KB buffer for template
            let result = unsafe {
                crate::llama_chat_apply_template(
                    std::ptr::null(), // Use model's built-in template
                    c_messages.as_ptr(),
                    messages.len(),
                    true, // add_ass=true
                    buffer.as_mut_ptr() as *mut u8,
                    buffer.len() as i32,
                )
            };

            if result > 0 {
                if let Ok(prompt) = std::ffi::CStr::from_bytes_until_nul(&buffer[..result as usize])
                {
                    if let Ok(prompt_str) = prompt.to_str() {
                        return prompt_str.to_string();
                    }
                }
            }
        }

        // Fallback to llama3 template
        let mut prompt = String::from("<|begin_of_text|>");
        for msg in messages {
            prompt.push_str(&format!(
                "<|start_header_id|>{}\n\n{}\n<|eot_id|>",
                msg.role, msg.content
            ));
        }
        prompt.push_str("<|start_header_id|>assistant\n\n");
        prompt
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // For non-mobile platforms, use simple concatenation as fallback
        let mut prompt = String::new();
        for msg in messages {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
        prompt.push_str("assistant: ");
        prompt
    }
}

fn handle_inference_task(
    stream: &mut MobileControlStream,
    task_id: String,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    top_k: i32,
    top_p: f32,
    repeat_penalty: f32,
) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        use crate::{
            gpuf_start_generation_async, GLOBAL_CONTEXT_PTR, GLOBAL_INFERENCE_MUTEX,
            GLOBAL_MODEL_PTR,
        };

        fn filter_control_tokens(text: &str) -> String {
            text.replace("<|end|>", "")
                .replace("<|start|>", "")
                .replace("<|channel|>", "")
                .replace("<|message|>", "")
                .replace("<|eot_id|>", "")
                .replace("<|start_header_id|>", "")
                .replace("<|end_header_id|>", "")
        }

        #[derive(Debug, Clone)]
        struct PhaseSplitter {
            phase: common::OutputPhase,
            carry: String,
        }

        impl Default for PhaseSplitter {
            fn default() -> Self {
                Self {
                    phase: common::OutputPhase::Final,
                    carry: String::new(),
                }
            }
        }

        impl PhaseSplitter {
            fn next_marker(rest: &str) -> Option<(usize, common::OutputPhase, usize)> {
                let mut best: Option<(usize, common::OutputPhase, usize)> = None;

                for (tag, to_phase) in [
                    ("<analysis>", common::OutputPhase::Analysis),
                    ("<think>", common::OutputPhase::Analysis),
                    ("<reasoning>", common::OutputPhase::Analysis),
                    ("<final>", common::OutputPhase::Final),
                ] {
                    if let Some(idx) = rest.find(tag) {
                        let cand = (idx, to_phase, tag.len());
                        best = Some(match best {
                            None => cand,
                            Some(cur) => {
                                if cand.0 < cur.0 {
                                    cand
                                } else {
                                    cur
                                }
                            }
                        });
                    }
                }

                for tag in ["</analysis>", "</think>", "</reasoning>", "</final>"] {
                    if let Some(idx) = rest.find(tag) {
                        let cand = (idx, common::OutputPhase::Final, tag.len());
                        best = Some(match best {
                            None => cand,
                            Some(cur) => {
                                if cand.0 < cur.0 {
                                    cand
                                } else {
                                    cur
                                }
                            }
                        });
                    }
                }

                for (pat, to_phase) in [
                    ("### Final", common::OutputPhase::Final),
                    ("Final:", common::OutputPhase::Final),
                    ("final:", common::OutputPhase::Final),
                    ("final", common::OutputPhase::Final),
                    ("### Answer", common::OutputPhase::Final),
                    ("### Reasoning", common::OutputPhase::Analysis),
                    ("Reasoning:", common::OutputPhase::Analysis),
                    ("Analysis:", common::OutputPhase::Analysis),
                    ("analysis:", common::OutputPhase::Analysis),
                    ("analysis", common::OutputPhase::Analysis),
                    ("### Answer", common::OutputPhase::Final),
                    ("Answer:", common::OutputPhase::Final),
                ] {
                    if rest.starts_with(pat) {
                        if (pat == "analysis" || pat == "final")
                            && rest.len() > pat.len()
                            && !rest[pat.len()..].starts_with([' ', '\n', '\r', '\t', ':'])
                        {
                        } else {
                            let cand = (0usize, to_phase, pat.len());
                            best = Some(match best {
                                None => cand,
                                Some(cur) => {
                                    if cand.0 < cur.0 {
                                        cand
                                    } else {
                                        cur
                                    }
                                }
                            });
                        }
                    }
                    let needle = format!("\n{}", pat);
                    if let Some(idx) = rest.find(&needle) {
                        let cand = (idx + 1, to_phase, pat.len());
                        best = Some(match best {
                            None => cand,
                            Some(cur) => {
                                if cand.0 < cur.0 {
                                    cand
                                } else {
                                    cur
                                }
                            }
                        });
                    }
                }

                best
            }

            fn split_tail_for_carry<'a>(tail: &'a str) -> (&'a str, &'a str) {
                let markers = [
                    "<analysis>",
                    "</analysis>",
                    "<think>",
                    "</think>",
                    "<reasoning>",
                    "</reasoning>",
                    "<final>",
                    "</final>",
                    "### Final",
                    "Final:",
                    "### Answer",
                    "final:",
                    "final",
                    "### Reasoning",
                    "Reasoning:",
                    "Analysis:",
                    "analysis:",
                    "analysis",
                    "### Answer",
                    "Answer:",
                ];
                let max_len = markers.iter().map(|s| s.len()).max().unwrap_or(0);

                let mut carry_len: usize = 0;
                let max_scan = (max_len.saturating_sub(1)).min(tail.len());

                for l in 1..=max_scan {
                    let start = tail.len() - l;
                    if !tail.is_char_boundary(start) {
                        continue;
                    }
                    let suf = &tail[start..];
                    if markers.iter().any(|m| m.starts_with(suf)) {
                        carry_len = l;
                    }
                    if markers.iter().any(|m| {
                        let nm = format!("\n{}", m);
                        nm.starts_with(suf)
                    }) {
                        carry_len = l;
                    }
                }

                if carry_len == 0 {
                    return (tail, "");
                }

                let split = tail.len() - carry_len;
                if !tail.is_char_boundary(split) {
                    return (tail, "");
                }
                (&tail[..split], &tail[split..])
            }

            fn phase(&self) -> common::OutputPhase {
                self.phase
            }

            fn push(&mut self, text: &str) -> Vec<(common::OutputPhase, String)> {
                if text.is_empty() {
                    return Vec::new();
                }

                let mut combined = String::new();
                if !self.carry.is_empty() {
                    combined.push_str(&self.carry);
                    self.carry.clear();
                }
                combined.push_str(text);

                let mut out: Vec<(common::OutputPhase, String)> = Vec::new();
                let mut pos: usize = 0;

                while pos < combined.len() {
                    let rest = &combined[pos..];

                    let Some((rel_pos, to_phase, marker_len)) = Self::next_marker(rest) else {
                        let tail = &combined[pos..];
                        let (safe, carry) = Self::split_tail_for_carry(tail);
                        if !safe.is_empty() {
                            out.push((self.phase, safe.to_string()));
                        }
                        self.carry = carry.to_string();
                        break;
                    };

                    let tag_pos = pos + rel_pos;

                    if tag_pos > pos {
                        let seg = &combined[pos..tag_pos];
                        if !seg.is_empty() {
                            out.push((self.phase, seg.to_string()));
                        }
                    }

                    if tag_pos + marker_len > combined.len() {
                        self.carry = combined[tag_pos..].to_string();
                        break;
                    }

                    self.phase = to_phase;
                    pos = tag_pos + marker_len;
                }
                out
            }
        }

        let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();

        let model_ptr = GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
        let ctx_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);

        if model_ptr.is_null() || ctx_ptr.is_null() {
            let result_command = CommandV1::InferenceResultChunk {
                task_id,
                seq: 0,
                delta: String::new(),
                phase: common::OutputPhase::Unknown,
                done: true,
                error: Some("Model not loaded - please load a model first".to_string()),
                prompt_tokens: 0,
                completion_tokens: 0,
                analysis_tokens: 0,
                final_tokens: 0,
            };
            common::write_command_sync(stream, &Command::V1(result_command))?;
            stream.flush().ok();
            return Ok(());
        }

        let prompt_c =
            std::ffi::CString::new(prompt).map_err(|e| anyhow!("Invalid prompt: {}", e))?;

        #[repr(C)]
        struct TokenCallbackState {
            stream: *mut MobileControlStream,
            task_id: String,
            seq: u32,
            buf: String,
            max_bytes: usize,
            buf_phase: common::OutputPhase,
            splitter: PhaseSplitter,
            prompt_tokens: u32,
            completion_tokens: u32,
            analysis_tokens: u32,
            final_tokens: u32,
            cancelled: AtomicBool,
        }

        extern "C" fn on_token(token: *const c_char, user_data: *mut std::ffi::c_void) {
            if token.is_null() || user_data.is_null() {
                return;
            }

            // SAFETY: `user_data` is created from a live `TokenCallbackState`
            // pointer immediately before calling `gpuf_start_generation_async`.
            // The generation call is synchronous for this callback and returns
            // before `cb_state` is dropped, so the pointer remains valid here.
            let state = unsafe { &mut *(user_data as *mut TokenCallbackState) };

            // Check if this task has been cancelled
            if let Some(slot) = WORKER_CANCELLED_TASK.get() {
                if let Ok(mut guard) = slot.lock() {
                    if guard.as_ref() == Some(&state.task_id) {
                        crate::gpuf_stop_generation(std::ptr::null_mut());
                        state.cancelled.store(true, Ordering::Relaxed);
                        *guard = None;
                        return;
                    }
                }
            }

            // SAFETY: The generation callback contract provides `token` as a
            // non-null, NUL-terminated C string for the duration of this call.
            let token_str = unsafe { std::ffi::CStr::from_ptr(token).to_str() };
            let Ok(token_str) = token_str else {
                return;
            };

            if token_str.is_empty() {
                return;
            }

            state.completion_tokens = state.completion_tokens.saturating_add(1);

            let filtered = filter_control_tokens(token_str);
            if filtered.is_empty() {
                return;
            }

            let segs = state.splitter.push(&filtered);
            for (phase, seg) in segs {
                if seg.is_empty() {
                    continue;
                }

                match phase {
                    common::OutputPhase::Analysis => {
                        state.analysis_tokens = state.analysis_tokens.saturating_add(1);
                    }
                    common::OutputPhase::Final => {
                        state.final_tokens = state.final_tokens.saturating_add(1);
                    }
                    common::OutputPhase::Unknown => {}
                }

                if state.buf.is_empty() {
                    state.buf_phase = phase;
                } else if state.buf_phase != phase {
                    let delta = std::mem::take(&mut state.buf);
                    let chunk = CommandV1::InferenceResultChunk {
                        task_id: state.task_id.clone(),
                        seq: state.seq,
                        delta,
                        phase: state.buf_phase,
                        done: false,
                        error: None,
                        prompt_tokens: state.prompt_tokens,
                        completion_tokens: state.completion_tokens,
                        analysis_tokens: state.analysis_tokens,
                        final_tokens: state.final_tokens,
                    };
                    state.seq = state.seq.wrapping_add(1);
                    // SAFETY: `state.stream` points to the active control stream passed to
                    // `gpuf_start_generation_async` and remains valid until that call returns.
                    let stream = unsafe { &mut *state.stream };
                    let _ = common::write_command_sync(stream, &Command::V1(chunk));
                    let _ = stream.flush();
                    state.buf_phase = phase;
                }

                state.buf.push_str(&seg);
                if state.buf.len() < state.max_bytes {
                    continue;
                }

                let delta = std::mem::take(&mut state.buf);
                let chunk = CommandV1::InferenceResultChunk {
                    task_id: state.task_id.clone(),
                    seq: state.seq,
                    delta,
                    phase: state.buf_phase,
                    done: false,
                    error: None,
                    prompt_tokens: state.prompt_tokens,
                    completion_tokens: state.completion_tokens,
                    analysis_tokens: state.analysis_tokens,
                    final_tokens: state.final_tokens,
                };
                state.seq = state.seq.wrapping_add(1);
                // SAFETY: `state.stream` points to the active control stream passed to
                // `gpuf_start_generation_async` and remains valid until that call returns.
                let stream = unsafe { &mut *state.stream };
                let _ = common::write_command_sync(stream, &Command::V1(chunk));
                let _ = stream.flush();
            }
        }

        let mut cb_state = TokenCallbackState {
            stream: stream as *mut MobileControlStream,
            task_id: task_id.clone(),
            seq: 0,
            buf: String::new(),
            max_bytes: 8,
            buf_phase: common::OutputPhase::Unknown,
            splitter: PhaseSplitter::default(),
            prompt_tokens: 0,
            completion_tokens: 0,
            analysis_tokens: 0,
            final_tokens: 0,
            cancelled: AtomicBool::new(false),
        };

        // SAFETY: `ctx_ptr` is checked above and `prompt_c` remains alive for
        // the full synchronous generation call. `cb_state` is passed as
        // `user_data` and is not moved or dropped until the call returns.
        let rc = unsafe {
            gpuf_start_generation_async(
                ctx_ptr,
                prompt_c.as_ptr(),
                max_tokens as i32,
                temperature,
                top_k,
                top_p,
                repeat_penalty,
                Some(on_token),
                (&mut cb_state as *mut TokenCallbackState) as *mut std::ffi::c_void,
            )
        };

        if rc < 0 {
            let result_command = CommandV1::InferenceResultChunk {
                task_id,
                seq: 0,
                delta: String::new(),
                phase: common::OutputPhase::Final,
                done: true,
                error: Some(format!("Inference failed: {}", rc)),
                prompt_tokens: 0,
                completion_tokens: 0,
                analysis_tokens: 0,
                final_tokens: 0,
            };
            common::write_command_sync(stream, &Command::V1(result_command))?;
            stream.flush().ok();
            // Clear any stale cancellation flag for this task
            if let Some(slot) = WORKER_CANCELLED_TASK.get() {
                if let Ok(mut guard) = slot.lock() {
                    *guard = None;
                }
            }
            return Ok(());
        }

        let was_cancelled = cb_state.cancelled.load(Ordering::Relaxed);

        if !cb_state.buf.is_empty() {
            let delta = std::mem::take(&mut cb_state.buf);
            let chunk = CommandV1::InferenceResultChunk {
                task_id: task_id.clone(),
                seq: cb_state.seq,
                delta,
                phase: cb_state.buf_phase,
                done: false,
                error: None,
                prompt_tokens: cb_state.prompt_tokens,
                completion_tokens: cb_state.completion_tokens,
                analysis_tokens: cb_state.analysis_tokens,
                final_tokens: cb_state.final_tokens,
            };
            cb_state.seq = cb_state.seq.wrapping_add(1);
            common::write_command_sync(stream, &Command::V1(chunk))?;
            stream.flush().ok();
        }

        let done_cmd = CommandV1::InferenceResultChunk {
            task_id,
            seq: cb_state.seq,
            delta: String::new(),
            phase: cb_state.splitter.phase(),
            done: true,
            error: if was_cancelled {
                Some("Inference cancelled by server".to_string())
            } else {
                None
            },
            prompt_tokens: cb_state.prompt_tokens,
            completion_tokens: cb_state.completion_tokens,
            analysis_tokens: cb_state.analysis_tokens,
            final_tokens: cb_state.final_tokens,
        };

        common::write_command_sync(stream, &Command::V1(done_cmd))?;
        stream.flush().ok();

        // Clear any stale cancellation flag for this task
        if let Some(slot) = WORKER_CANCELLED_TASK.get() {
            if let Ok(mut guard) = slot.lock() {
                *guard = None;
            }
        }

        Ok(())
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // For non-mobile platforms, return an error as this module is for mobile workers only
        return Err(anyhow!(
            "handle_inference_task is only implemented for mobile platforms (Android/iOS)"
        ));
    }
}

pub async fn stop_global_worker() {
    if let Some(stop) = WORKER_STOP_SIGNAL.get() {
        stop.store(true, Ordering::Relaxed);
    }

    if let Some(m) = WORKER_TCP_STREAM.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
    if let Some(m) = WORKER_SERVER_ADDR.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
    if let Some(m) = WORKER_CONTROL_PORT.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
    if let Some(m) = WORKER_CLIENT_ID.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
}

pub async fn get_worker_status() -> Result<String> {
    if get_tcp_stream().is_some() {
        Ok("Worker is running".to_string())
    } else {
        Ok("Worker not available".to_string())
    }
}
