use super::*;
#[cfg(not(target_os = "macos"))]
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::{self, llama_engine::LlamaEngine};
use crate::util::system_info::{
    collect_device_info, collect_system_info, get_engine_models, pull_ollama_model, run_model,
};
use anyhow::Result;
use common::{
    format_bytes, format_duration, join_streams, read_command, write_command, Command, CommandV1,
    EngineType as ClientEngineType, Model, OsType, SystemInfo, MAX_MESSAGE_SIZE,
};
use tokio::io::AsyncWriteExt;

use futures_util::StreamExt;

use bytes::BytesMut;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
#[allow(unused_imports)]
use std::marker::PhantomData;
use std::net::ToSocketAddrs;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::interval;
#[cfg(not(target_os = "android"))]
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, ServerName},
        ClientConfig, RootCertStore,
    },
    TlsConnector,
};
use tracing::{debug, error, info, warn};

// Filter internal GGUF control tokens from streaming output
fn filter_control_tokens(text: &str) -> String {
    // Skip everything that looks like internal thinking process
    if text.contains("analysis") || 
       text.contains("The user is speaking") ||
       text.contains("Means \"") ||
       text.contains("The assistant should") ||
       text.contains("We need to") ||
       text.contains("Thus produce") ||
       text.contains("Ok produce answer") ||
       text.contains("<assistant") ||
       text.contains("<|channel|>") ||
       text.contains("<|start|>") {
        return String::new();
    }
    
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    let mut buffer = String::new();
    
    while let Some(ch) = chars.next() {
        buffer.push(ch);
        
        // Check for any control token patterns
        if buffer.contains("<|") {
            // Skip until we find a safe point
            while let Some(c) = chars.next() {
                buffer.push(c);
                if buffer.ends_with(">") {
                    // Check if this was a control token
                    if buffer.contains("<|channel|>") || 
                       buffer.contains("<|start|>") || 
                       buffer.contains("<|end|>") || 
                       buffer.contains("<|message|>") {
                        buffer.clear();
                        break;
                    }
                    // If it's not a recognized control token, keep it
                    let safe_end = buffer.find('>').unwrap_or(buffer.len()) + 1;
                    result.push_str(&buffer[..safe_end]);
                    buffer.clear();
                    break;
                }
                if buffer.len() > 50 {
                    // Safety: prevent infinite growth
                    buffer.clear();
                    break;
                }
            }
            continue;
        }
        
        // Flush safe content periodically
        if buffer.len() > 20 {
            let safe_end = buffer.find('<').unwrap_or(buffer.len());
            if safe_end > 0 {
                result.push_str(&buffer[..safe_end]);
                buffer.drain(0..safe_end);
            }
        }
    }
    
    // Flush remaining buffer
    result.push_str(&buffer);
    
    // Final cleanup
    result
        .replace("<|end|>", "")
        .replace("<|start|>", "")
        .replace("<|channel|>", "")
        .replace("<|message|>", "")
}

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

const CURRENT_VERSION: u32 = 1;

impl TCPWorker {
    /// Execute inference task using local LLM engine (Android specific)

    async fn execute_inference_task(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    ) -> Result<String> {
        #[cfg(not(target_os = "android"))]
        {
            let engine_guard = self.engine.lock().await;
            let engine = engine_guard
                .as_ref()
                .ok_or_else(|| anyhow!("Engine not initialized"))?;

            match engine {
                AnyEngine::Llama(llama) => {
                    let sampling = crate::llm_engine::llama_engine::SamplingParams {
                        temperature: temperature,
                        top_k: top_k as i32,
                        top_p: top_p,
                        repeat_penalty: repeat_penalty,
                        repeat_last_n: repeat_last_n,
                        seed: 0,
                        min_keep: min_keep as usize,
                    };

                    let (text, _prompt_tokens, _completion_tokens) = llama
                        .generate_with_cached_model_sampling(prompt, max_tokens as usize, &sampling)
                        .await?;
                    Ok(text)
                }

                _ => Err(anyhow!(
                    "execute_inference_task is only supported for LLAMA engine"
                )),
            }
        }

        #[cfg(target_os = "android")]
        {
            use crate::{
                gpuf_generate_final_solution_text, GLOBAL_CONTEXT_PTR, GLOBAL_INFERENCE_MUTEX,
                GLOBAL_MODEL_PTR,
            };
            use std::ffi::CString;
            use std::sync::atomic::Ordering;

            // Acquire global inference lock to prevent concurrent execution
            let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();

            // Get global model and context pointers
            let model_ptr = GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
            let context_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);

            if model_ptr.is_null() || context_ptr.is_null() {
                return Err(anyhow!("Model not loaded - please load a model first"));
            }

            // Convert prompt to CString
            let prompt_cstr = CString::new(prompt).map_err(|e| anyhow!("Invalid prompt: {}", e))?;

            // Create output buffer
            let mut output = vec![0u8; 4096];

            // Execute inference using existing JNI function
            // SAFETY: We're calling an FFI function with valid pointers:
            // - model_ptr and context_ptr are checked for null above
            // - prompt_cstr.as_ptr() is a valid C string pointer
            // - output buffer is properly sized and mutable
            let result = gpuf_generate_final_solution_text(
                model_ptr,
                context_ptr,
                prompt_cstr.as_ptr(),
                max_tokens as i32,
                output.as_mut_ptr() as *mut std::os::raw::c_char,
                output.len() as i32,
            );

            if result > 0 {
                let output_str = unsafe {
                    std::ffi::CStr::from_ptr(output.as_ptr() as *const std::os::raw::c_char)
                        .to_str()
                        .map_err(|e| anyhow!("Invalid UTF-8 in output: {}", e))?
                };
                Ok(output_str.to_string())
            } else {
                Err(anyhow!("Inference failed with code: {}", result))
            }
        }
    }

    async fn stream_inference_task_to_server(
        &self,
        task_id: String,
        prompt: String,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    ) -> Result<()> {
        #[cfg(not(target_os = "android"))]
        {
            let engine_guard = self.engine.lock().await;
            let engine = engine_guard
                .as_ref()
                .ok_or_else(|| anyhow!("Engine not initialized"))?;

            let AnyEngine::Llama(llama) = engine else {
                return Err(anyhow!(
                    "stream_inference_task_to_server is only supported for LLAMA engine"
                ));
            };

            let sampling = crate::llm_engine::llama_engine::SamplingParams {
                temperature,
                top_k: top_k as i32,
                top_p,
                repeat_penalty,
                repeat_last_n,
                seed: 0,
                min_keep: min_keep as usize,
            };

            let prompt_tokens: u32 = {
                let prompt = prompt.clone();
                let cached_model = llama
                    .cached_model
                    .as_ref()
                    .ok_or_else(|| anyhow!("Model not loaded - call load_model() first"))?
                    .clone();

                tokio::task::spawn_blocking(move || {
                    use llama_cpp_2::model::AddBos;

                    let model_guard = cached_model
                        .lock()
                        .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;

                    let tokens = model_guard
                        .str_to_token(&prompt, AddBos::Always)
                        .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;
                    Ok::<u32, anyhow::Error>(tokens.len().min(u32::MAX as usize) as u32)
                })
                .await??
            };

            let mut stream = llama
                .stream_with_cached_model_sampling(&prompt, max_tokens as usize, &sampling)
                .await?;

            let mut stream = Box::pin(stream);

            let max_bytes: usize = self.args.stream_chunk_bytes.max(1);
            let mut seq: u32 = 0;
            let mut buf = String::new();
            let mut completion_tokens: u32 = 0;

            let mut cancelled_early = false;
            loop {
                {
                    let cancelled = self.cancel_state.cancelled.lock().await;
                    if cancelled.contains(&task_id) {
                        cancelled_early = true;
                        debug!(task_id = %task_id, "Cancellation observed in stream loop");
                        break;
                    }
                }

                tokio::select! {
                    _ = self.cancel_state.notify.notified() => {
                        let cancelled = self.cancel_state.cancelled.lock().await;
                        if cancelled.contains(&task_id) {
                            cancelled_early = true;
                            debug!(task_id = %task_id, "Cancellation notified during streaming");
                            break;
                        }
                    }
                    piece_res = stream.next() => {
                        let Some(piece_res) = piece_res else {
                            break;
                        };
                        let piece = piece_res?;
                        let filtered = filter_control_tokens(&piece);
                        // Each streamed `piece` corresponds to (at most) one generated token.
                        // Never count bytes/chars here, otherwise completion_tokens can greatly exceed max_tokens.
                        completion_tokens = completion_tokens.saturating_add(1);

                        if !filtered.is_empty() {
                            buf.push_str(&filtered);

                            if buf.len() >= max_bytes {
                                let delta = std::mem::take(&mut buf);
                                let chunk = CommandV1::InferenceResultChunk {
                                    task_id: task_id.clone(),
                                    seq,
                                    delta,
                                    done: false,
                                    error: None,
                                    prompt_tokens,
                                    completion_tokens,
                                };
                                self.send_command(chunk).await?;
                                seq = seq.wrapping_add(1);
                            }
                        }
                    }
                }
            }

            if !buf.is_empty() {
                let chunk = CommandV1::InferenceResultChunk {
                    task_id: task_id.clone(),
                    seq,
                    delta: buf,
                    done: false,
                    error: None,
                    prompt_tokens,
                    completion_tokens,
                };
                self.send_command(chunk).await?;
                seq = seq.wrapping_add(1);
            }

            let done_chunk = CommandV1::InferenceResultChunk {
                task_id: task_id.clone(),
                seq,
                delta: String::new(),
                done: true,
                error: None,
                prompt_tokens,
                completion_tokens,
            };
            self.send_command(done_chunk).await?;

            if cancelled_early {
                debug!(task_id = %task_id, "Sent done chunk after cancellation");
            }

            {
                let mut cancelled = self.cancel_state.cancelled.lock().await;
                cancelled.remove(&task_id);
            }
            return Ok(());
        }

        #[cfg(target_os = "android")]
        {
            let _ = (
                task_id,
                prompt,
                max_tokens,
                temperature,
                top_k,
                top_p,
                repeat_penalty,
                repeat_last_n,
                min_keep,
            );
            Err(anyhow!("Android streaming is not implemented"))
        }
    }

    fn build_chat_prompt_fallback(&self, messages: &[common::ChatMessage]) -> String {
        let template = std::env::var("CHAT_TEMPLATE").unwrap_or_else(|_| "simple".to_string());
        match template.to_ascii_lowercase().as_str() {
            "chatml" => {
                let mut prompt = String::new();
                for msg in messages {
                    prompt.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", msg.role, msg.content));
                }
                prompt.push_str("<|im_start|>assistant\n");
                prompt
            }
            "llama3" => {
                let mut prompt = String::from("<|begin_of_text|>");
                for msg in messages {
                    prompt.push_str(&format!(
                        "<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>",
                        msg.role, msg.content
                    ));
                }
                prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
                prompt
            }
            _ => {
                let mut prompt = String::new();
                for msg in messages {
                    let role = match msg.role.as_str() {
                        "user" => "Human",
                        "assistant" => "Assistant",
                        _ => "System",
                    };
                    prompt.push_str(&format!("{}: {}\n\n", role, msg.content));
                }
                prompt.push_str("Assistant: ");
                prompt
            }
        }
    }

    /// Send command to server
    async fn send_command(&self, command: CommandV1) -> Result<()> {
        use common::{write_command, Command};

        let command = Command::V1(command);
        let mut writer = self.writer.lock().await;
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }
    pub async fn new(args: Args) -> Result<Self> {
       
        let (device_info, device_memtotal_mb) = match collect_device_info().await {
            Ok(info) => info,
            Err(e) => {
                error!("Failed to collect device info: {}", e);
                return Err(anyhow!("Failed to collect device info"));
            }
        };

        if device_info.num == 0 {
            error!(" device is empty");
            return Err(anyhow!(" device is empty"));
        }

        info!("Debug: Engine type from args: {:?}", args.engine_type);

        let os_type = if cfg!(target_os = "macos") {
            OsType::MACOS
        } else if cfg!(target_os = "windows") {
            OsType::WINDOWS
        } else if cfg!(target_os = "linux") {
            OsType::LINUX
        } else if cfg!(target_os = "android") {
            OsType::ANDROID
        } else if cfg!(target_os = "ios") {
            OsType::IOS
        } else {
            OsType::NONE
        };
        let engine_type = match args.engine_type {
            EngineType::VLLM => ClientEngineType::Vllm,
            EngineType::OLLAMA => ClientEngineType::Ollama,
            EngineType::LLAMA => ClientEngineType::Llama,
        };
        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
        let mut engine: Option<AnyEngine> = None;
        #[cfg(any(target_os = "macos", target_os = "android"))]
        let mut engine: Option<()> = None;
        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
        {
            if args.engine_type == EngineType::VLLM {
                let mut llvm_worker = llm_engine::create_engine(
                    args.engine_type.clone(),
                    args.hugging_face_hub_token.clone(),
                    args.chat_template_path.clone(),
                );
                match llvm_worker.init().await {
                    Ok(_) => info!("VLLM init success"),
                    Err(e) => error!("VLLM init failed: {}", e),
                }
                engine = Some(llvm_worker);
            } else if args.engine_type == EngineType::LLAMA {
                // Initialize LLAMA engine (single shared instance for both worker and HTTP server)
                let mut llama_worker = if let Some(model_path) = &args.llama_model_path {
                    // Use provided model path
                    info!("Creating LLAMA engine with model: {}", model_path);
                    llm_engine::AnyEngine::Llama(LlamaEngine::with_config(
                        model_path.clone(),
                        4096, // context size
                        args.n_gpu_layers, // GPU layers
                    ))
                } else {
                    // Create engine without model (will be set later)
                    info!("Creating LLAMA engine without model (will be set later)");
                    llm_engine::create_engine(
                        args.engine_type.clone(),
                        args.hugging_face_hub_token.clone(),
                        args.chat_template_path.clone(),
                    )
                };

                // Initialize the engine (only once)
                match llama_worker.init().await {
                    Ok(_) => {
                        info!("LLAMA engine init success");
                        // Start worker
                        match llama_worker.start_worker().await {
                            Ok(_) => info!("LLAMA worker started"),
                            Err(e) => error!("LLAMA worker start failed: {}", e),
                        }
                    }
                    Err(e) => error!("LLAMA init failed: {}", e),
                }

                // Clone the engine for HTTP server (same instance, shared data via Arc)
                let server_engine = match llama_worker {
                    AnyEngine::Llama(ref e) => e.clone(),
                    _ => unreachable!(),
                };

                // Store engine for GPUFabric worker
                engine = Some(llama_worker);

                // Start local HTTP API server for LLAMA (for proxy forwarding)
                // Use the SAME engine instance for both worker and HTTP server
                let local_addr = args.local_addr.clone();
                let local_port = args.local_port;
                let local_addr_clone = local_addr.clone();
                info!(
                    "Starting LLAMA HTTP API server on {}:{}",
                    local_addr, local_port
                );

                use crate::llm_engine::llama_server::start_server;
                use std::sync::Arc;
                use tokio::sync::RwLock;

                // Wrap the shared engine in Arc<RwLock> for HTTP server
                let engine_arc = Arc::new(RwLock::new(server_engine));

                // Spawn server in background
                tokio::spawn(async move {
                    if let Err(e) = start_server(engine_arc, &local_addr_clone, local_port).await {
                        error!("LLAMA HTTP server error: {}", e);
                    }
                });

                info!(
                    "LLAMA HTTP API server started successfully on {}:{}",
                    local_addr, local_port
                );
            }
        }
        #[cfg(target_os = "macos")]
        {
            // if args.engine_type == EngineType::OLLAMA {
            //     let mut llvm_worker = llm_engine::create_engine(
            //         args.engine_type.clone(),
            //         args.hugging_face_hub_token.clone(),
            //         args.chat_template_path.clone(),
            //     );
            //     match llvm_worker.init().await {
            //         Ok(_) => info!("VLLM init success"),
            //         Err(e) => error!("VLLM init failed: {}", e),
            //     }
            //     engine = Some(llvm_worker);
            // }

            if args.engine_type == EngineType::OLLAMA {
                if let Err(e) = check_and_restart_ollama().await {
                    error!("Failed to manage Ollama process: {}", e);
                    // Decide whether to return error or continue without Ollama
                }
            }
        }
        let device_memtotal_gb = device_memtotal_mb as u32;
        let device_total_tflops = device_info.total_tflops as u32;
        
        let addr_str = format!("{}:{}", args.server_addr, args.control_port);
        let addr = addr_str.to_socket_addrs()?.next().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid server address or port",
            )
        })?;
        let ip_addr = addr.ip();
        let control_stream = TcpStream::connect(addr).await?;

        info!("Connected to control port.");
        
        let (reader, writer) = tokio::io::split(control_stream);
        //network monitor
        let network_monitor = Arc::new(Mutex::new(
            SessionNetworkMonitor::new(None).expect("Failed to create network monitor"),
        ));
        //system info
        let (cpu_useage, mem_useage, disk_useage, _computer_name) = collect_system_info().await?;

        let stats = network_monitor.lock().await.refresh().unwrap_or((0, 0));
        let worker = Self {
            addr: ip_addr,
            #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
            engine: Arc::new(Mutex::new(engine)),
            #[cfg(any(target_os = "macos", target_os = "android"))]
            _engine: PhantomData,
            //TODO: only one device
            devices_info: Arc::new(vec![device_info]),
            reader: Arc::new(Mutex::new(reader)),
            writer: Arc::new(Mutex::new(writer)),
            system_info: Arc::new(SystemInfo {
                cpu_usage: cpu_useage,
                memory_usage: mem_useage,
                disk_usage: disk_useage,
                network_rx: stats.0,
                network_tx: stats.1,
            }),
            client_id: args.client_id.expect("client_id is required"),
            device_memtotal_gb,
            device_total_tflops,
            os_type,
            engine_type,
            args,
            network_monitor,
            cancel_state: Arc::new(CancelState {
                cancelled: Mutex::new(HashSet::new()),
                notify: tokio::sync::Notify::new(),
            }),
        };
        Ok(worker)
    }

    #[allow(dead_code)]
    pub async fn system_info(&self) -> Result<SystemInfo> {
        let (cpu_useage, mem_useage, disk_useage, _computer_name) = collect_system_info().await?; //network info
        let mut network_info = self.network_monitor.lock().await;
        let stats = network_info.refresh().unwrap_or((0, 0));
        Ok(SystemInfo {
            cpu_usage: cpu_useage,
            memory_usage: mem_useage,
            disk_usage: disk_useage,
            network_rx: stats.0,
            network_tx: stats.1,
        })
    }
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
async fn check_and_restart_ollama() -> Result<()> {
    use std::process::Stdio;
    use tokio::process::Command;

    // Check if Ollama is running
    let check_status = Command::new("pgrep")
        .arg("Ollama")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match check_status {
        Ok(status) if status.success() => {
            // Ollama is running, kill it first
            info!("Ollama is running, restarting...");
            let _ = Command::new("pkill")
                .arg("Ollama")
                .status()
                .await
                .map_err(|e| {
                    error!("Failed to kill Ollama process: {}", e);
                    anyhow::anyhow!("Failed to kill Ollama process: {}", e)
                })?;

            // Give it a moment to shut down
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        _ => {
            info!("Ollama is not running, will start it");
        }
    }

    // Start Ollama with environment variables
    let mut cmd = Command::new("ollama");
    cmd.arg("serve")
        .env("OLLAMA_HOST", "0.0.0.0")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Start the process in the background
    cmd.spawn().map_err(|e| {
        error!("Failed to start Ollama: {}", e);
        anyhow::anyhow!("Failed to start Ollama: {}", e)
    })?;

    // Wait for Ollama to be ready
    let max_retries = 10;
    let mut retry_count = 0;
    let client = reqwest::Client::new();

    while retry_count < max_retries {
        match client
            .get("http://localhost:11434/api/tags")
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                info!("Ollama is ready");
                return Ok(());
            }
            Err(e) => {
                debug!(
                    "Ollama not ready yet (attempt {}/{}): {}",
                    retry_count + 1,
                    max_retries,
                    e
                );
            }
            _ => {}
        }

        retry_count += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Err(anyhow::anyhow!(
        "Failed to start Ollama: timeout after {} retries",
        max_retries
    ))
}

impl WorkerHandle for TCPWorker {
    fn login(&self) -> impl Future<Output = Result<()>> + Send {
        async move {
            info!("🔧 Starting login process...");
            let login_cmd = CommandV1::Login {
                version: CURRENT_VERSION,
                auto_models: self.args.auto_models,
                os_type: self.os_type.clone(),
                client_id: self.client_id.clone(),
                system_info: (*self.system_info).clone(),
                device_memtotal_gb: self.device_memtotal_gb,
                device_total_tflops: self.device_total_tflops,
                devices_info: self.devices_info.as_ref().clone(),
            };
            info!("📤 About to write login command to server...");
            match write_command(&mut *self.writer.lock().await, &Command::V1(login_cmd)).await {
                Ok(_) => {
                    info!("✅ Login command written successfully");
                    Ok(())
                }
                Err(e) => {
                    error!("❌ Failed to write login command: {}", e);
                    Err(e)
                }
            }
        }
    }

    fn model_task(&self, get_last_models: &str) -> impl Future<Output = Result<()>> + Send {
        async move {
            let writer_clone = Arc::clone(&self.writer);
            let client_id = Arc::new(self.client_id.clone());
            // let device_memtotal_gb = self.device_memtotal_gb;
            // let auto_models = self.args.auto_models;
            let engine_type = self.engine_type.clone();

            if self.args.auto_models {
                match engine_type {
                    common::EngineType::Ollama => {
                        pull_ollama_model(get_last_models, self.args.local_port).await?
                    }
                    common::EngineType::Vllm => {
                        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
                        if let Some(_engine) = self.engine.lock().await.as_mut() {
                            // Engine functionality disabled in lightweight version
                        }
                    }
                    _ => {}
                }
            }
            let local_port = self.args.local_port;
            let devices_info = self.devices_info.clone();
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(300)); // Send heartbeat every 10 seconds
                loop {
                    interval.tick().await;

                    let models: Vec<common::Model> = match engine_type {
                        common::EngineType::Ollama => match get_engine_models(local_port).await {
                            Ok(models) => {
                                info!(
                                    "Successfully fetched {} models from Ollama.",
                                    models.len()
                                );
                                models
                            }
                            Err(e) => {
                                warn!(
                                    "Could not fetch models from Ollama: {}. This is okay if Ollama is not running.",
                                    e
                                );
                                Vec::new()
                            }
                        },
                        common::EngineType::Llama => {
                            let current_model_path = crate::MODEL_STATUS
                                .lock()
                                .ok()
                                .and_then(|s| s.current_model.clone());

                            match current_model_path {
                                Some(model_path) => {
                                    let model_id = derive_model_id_from_path(&model_path);
                                    vec![Model {
                                        id: model_id,
                                        object: "model".to_string(),
                                        created: 0,
                                        owned_by: "gpuf-c".to_string(),
                                    }]
                                }
                                None => Vec::new(),
                            }
                        }
                        _ => Vec::new(),
                    };
                    let model_cmd = CommandV1::ModelStatus {
                        client_id: *client_id,
                        models,
                        auto_models_device: devices_info.clone().to_vec(),
                    };
                    if let Err(e) =
                        write_command(&mut *writer_clone.lock().await, &Command::V1(model_cmd)).await
                    {
                        error!(
                            "Failed to send model status (connection may be closed): {}",
                            e
                        );
                        break;
                    }
                }
            });
            Ok(())
        }
    }

    fn heartbeat_task(&self) -> impl Future<Output = Result<()>> + Send {
        async move {
            let writer_clone = Arc::clone(&self.writer);
            let client_id = Arc::new(self.client_id.clone());
            let network_monitor = Arc::clone(&self.network_monitor);
            // network_monitor.lock().await.update();
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(120)); // Send heartbeat every 120 seconds

                loop {
                    interval.tick().await;

                    let (cpu_usage, memory_usage, disk_usage, _computer_name) =
                        match collect_system_info().await {
                            Ok(info) => info,
                            Err(e) => {
                                error!("Failed to collect system info: {}", e);
                                continue;
                            }
                        };

                    // device_info should be real-time for monitoring
                    let (device_info, device_memtotal_mb) = match collect_device_info().await {
                        Ok(info) => info,
                        Err(e) => {
                            error!("Failed to collect device info: {}", e);
                            (DevicesInfo::default(), 0)
                        }
                    };

                    // TODO: device_info is remote device info
                    info!("Sending heartbeat to server cpu_usage {}% memory_usage {}% disk_usage {}% device_memtotal {}mb", cpu_usage, memory_usage, disk_usage, device_memtotal_mb);

                    let (stats, session_stats) = {
                        let mut monitor = network_monitor.lock().await;
                        let stats = monitor.refresh().unwrap_or((0, 0));
                        let session_stats = monitor.get_session_stats();
                        (stats, session_stats)
                    };
                    info!(
                        "Network stats - Current: up {} down {} | Session Total: up {} down {} | Duration: {} ", 
                        format_bytes!(stats.1),
                        format_bytes!(stats.0),
                        format_bytes!(session_stats.1),
                        format_bytes!(session_stats.0),
                        format_duration!(session_stats.2.as_secs())
                    );

                    let mut writer = writer_clone.lock().await;
                    if let Err(e) = write_command(
                        &mut *writer,
                        &Command::V1(CommandV1::Heartbeat {
                            client_id: *client_id,
                            system_info: SystemInfo {
                                cpu_usage: cpu_usage,
                                memory_usage: memory_usage,
                                disk_usage: disk_usage,
                                network_rx: stats.0,
                                network_tx: stats.1,
                            },
                            // TODO: devices_info device_count device_total_tflops and device_memtotal_gb is single device
                            device_memtotal_gb: device_info.memtotal_gb as u32,
                            device_total_tflops: device_info.total_tflops as u32,
                            device_count: device_info.num as u16,
                            devices_info: vec![device_info],
                        }),
                    )
                    .await
                    {
                        error!("Failed to send heartbeat: {}", e);
                        break;
                    }
                }
            });
            Ok(())
        }
    }

    fn handler(&self) -> impl Future<Output = Result<()>> + Send {
        async move {
            let mut buf = BytesMut::with_capacity(MAX_MESSAGE_SIZE);
            loop {
                match read_command(&mut *self.reader.lock().await, &mut buf).await? {
                    Command::V1(cmd_v1) => {
                        match cmd_v1 {
                            CommandV1::CancelInference { task_id } => {
                                debug!(task_id = %task_id, "Received CancelInference");
                                {
                                    let mut cancelled = self.cancel_state.cancelled.lock().await;
                                    cancelled.insert(task_id);
                                }
                                self.cancel_state.notify.notify_waiters();
                            }
                            CommandV1::LoginResult {
                                success,
                                pods_model,
                                error,
                            } => {
                                if success {
                                    if pods_model.is_empty() {
                                        error!("Received empty models from server");
                                        return Err(anyhow!(
                                            "device is not compatible with the model"
                                        ));
                                    }
                                    //TODO models is local models
                                    for pod_model in pods_model {
                                        if let Some(model_name) = pod_model.model_name {
                                            self.model_task(&model_name).await?;
                                        }
                                    }
                                    self.heartbeat_task().await?;
                                    debug!("Successfully logged in.");
                                    continue;
                                } else {
                                    error!("Login failed: {}", error.unwrap_or_default());
                                    return Err(anyhow!("Login failed"));
                                }
                            }
                            CommandV1::PullModelResult { pods_model, error } => {
                                if error.is_some() {
                                    error!("Pull model failed: {}", error.unwrap_or_default());
                                    return Err(anyhow!("Pull model failed"));
                                }
                                if pods_model.is_empty() {
                                    error!("device is not compatible with the model");
                                    return Err(anyhow!("device is not compatible with the model"));
                                }
                                // TODO: pull model
                                for pod_model in pods_model {
                                    if let Some(model_name) = pod_model.model_name {
                                        match self.engine_type {
                                            common::EngineType::Ollama => {
                                                pull_ollama_model(&model_name, self.args.local_port)
                                                    .await?
                                            }
                                            common::EngineType::Vllm => {
                                                #[cfg(all(
                                                    not(target_os = "macos"),
                                                    not(target_os = "android")
                                                ))]
                                                if let Some(_engine) =
                                                    self.engine.lock().await.as_mut()
                                                {
                                                    // Engine functionality disabled in lightweight version
                                                }
                                            }
                                            _ => {}
                                        }
                                        match run_model(
                                            self.args.local_port,
                                            &model_name,
                                            "hello world",
                                        )
                                        .await
                                        {
                                            Ok(output) => {
                                                info!("Model {} output: {}", model_name, output)
                                            }
                                            Err(e) => error!("run_model Error: {}", e),
                                        }
                                    }
                                }
                            }
                            CommandV1::RequestNewProxyConn { proxy_conn_id } => {
                                info!(
                                    "Received request for new proxy connection: {:?}",
                                    proxy_conn_id
                                );
                                let args_clone = self.args.clone();
                                let cert_chain_path_clone = self.args.cert_chain_path.clone();
                                let addr_clone = self.addr;
                                tokio::spawn(async move {
                                    if let Err(e) = create_proxy_connection(
                                        args_clone,
                                        addr_clone,
                                        proxy_conn_id,
                                        cert_chain_path_clone,
                                    )
                                    .await
                                    {
                                        error!("Failed to create proxy connection: {}", e);
                                    }
                                });
                            }
                            CommandV1::ChatInferenceTask {
                                task_id,
                                model: _model,
                                messages,
                                max_tokens,
                                temperature,
                                top_k,
                                top_p,
                                repeat_penalty,
                                repeat_last_n,
                                min_keep,
                            } => {
                                let prompt = {
                                    #[cfg(target_os = "android")]
                                    {
                                        self.build_chat_prompt_fallback(&messages)
                                    }

                                    #[cfg(not(target_os = "android"))]
                                    {
                                        let cached_model = {
                                            let engine_guard = self.engine.lock().await;
                                            let engine = engine_guard
                                                .as_ref()
                                                .ok_or_else(|| anyhow!("Engine not initialized"))?;

                                            let AnyEngine::Llama(llama) = engine else {
                                                return Err(anyhow!(
                                                    "ChatInferenceTask is only supported for LLAMA engine"
                                                ));
                                            };

                                            llama.cached_model
                                                .as_ref()
                                                .ok_or_else(|| {
                                                    anyhow!("Model not loaded - call load_model() first")
                                                })?
                                                .clone()
                                        };

                                        let messages_for_fallback = messages.clone();
                                        match tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                                            use llama_cpp_2::model::LlamaChatMessage;

                                            let model_guard = cached_model
                                                .lock()
                                                .map_err(|e| anyhow!("Failed to lock model: {:?}", e))?;

                                            let tmpl = model_guard
                                                .chat_template(None)
                                                .map_err(|e| anyhow!("Failed to get chat template: {:?}", e))?;

                                            let mut chat = Vec::with_capacity(messages_for_fallback.len());
                                            for m in messages_for_fallback {
                                                let msg = LlamaChatMessage::new(m.role, m.content)
                                                    .map_err(|e| anyhow!("Failed to build chat message: {:?}", e))?;
                                                chat.push(msg);
                                            }

                                            model_guard
                                                .apply_chat_template(&tmpl, &chat, true)
                                                .map_err(|e| anyhow!("Failed to apply chat template: {:?}", e))
                                        })
                                        .await
                                        {
                                            Ok(Ok(p)) => p,
                                            _ => self.build_chat_prompt_fallback(&messages),
                                        }
                                    }
                                };
                                let result = self
                                    .stream_inference_task_to_server(
                                        task_id.clone(),
                                        prompt,
                                        max_tokens,
                                        temperature,
                                        top_k,
                                        top_p,
                                        repeat_penalty,
                                        repeat_last_n,
                                        min_keep,
                                    )
                                    .await;

                                if let Err(e) = result {
                                    let chunk = CommandV1::InferenceResultChunk {
                                        task_id,
                                        seq: 0,
                                        delta: String::new(),
                                        done: true,
                                        completion_tokens: 0,
                                        prompt_tokens: 0,
                                        error: Some(e.to_string()),
                                    };
                                    self.send_command(chunk).await?;
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
                                repeat_last_n,
                                min_keep,
                            } => {
                                info!("Received inference task: {} max_tokens: {}", task_id, max_tokens);

                                let start_time = std::time::Instant::now();

                                #[cfg(not(target_os = "android"))]
                                {
                                    let result = self
                                        .stream_inference_task_to_server(
                                            task_id.clone(),
                                            prompt.clone(),
                                            max_tokens,
                                            temperature,
                                            top_k,
                                            top_p,
                                            repeat_penalty,
                                            repeat_last_n,
                                            min_keep,
                                        )
                                        .await;

                                    let _execution_time = start_time.elapsed().as_millis() as u64;
                                    if let Err(e) = result {
                                        let chunk = CommandV1::InferenceResultChunk {
                                            task_id,
                                            seq: 0,
                                            delta: String::new(),
                                            done: true,
                                            completion_tokens: 0,
                                            prompt_tokens: 0,
                                            error: Some(e.to_string()),
                                        };
                                        self.send_command(chunk).await?;
                                    }
                                }

                                #[cfg(target_os = "android")]
                                {
                                    let result = self
                                        .execute_inference_task(
                                            &prompt,
                                            max_tokens,
                                            temperature,
                                            top_k,
                                            top_p,
                                            repeat_penalty,
                                            repeat_last_n,
                                            min_keep,
                                        )
                                        .await;

                                    let _execution_time = start_time.elapsed().as_millis() as u64;

                                    match result {
                                        Ok(output) => {
                                            let mut seq: u32 = 0;
                                            let max_bytes: usize = self.args.stream_chunk_bytes.max(1);
                                            let mut start: usize = 0;
                                            while start < output.len() {
                                                let mut end = (start + max_bytes).min(output.len());
                                                while end < output.len()
                                                    && !output.is_char_boundary(end)
                                                {
                                                    end -= 1;
                                                }
                                                if end == start {
                                                    end = output
                                                        .char_indices()
                                                        .nth(1)
                                                        .map(|(i, _)| i)
                                                        .unwrap_or(output.len());
                                                }

                                                let delta = output[start..end].to_string();
                                                let chunk = CommandV1::InferenceResultChunk {
                                                    task_id: task_id.clone(),
                                                    seq,
                                                    delta,
                                                    done: false,
                                                    error: None,
                                                    prompt_tokens: 0,
                                                    completion_tokens: 0,
                                                };
                                                self.send_command(chunk).await?;
                                                seq = seq.wrapping_add(1);
                                                start = end;
                                            }

                                            let done_chunk = CommandV1::InferenceResultChunk {
                                                task_id,
                                                seq,
                                                delta: String::new(),
                                                done: true,
                                                error: None,
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            self.send_command(done_chunk).await?;
                                        }
                                        Err(e) => {
                                            let chunk = CommandV1::InferenceResultChunk {
                                                task_id,
                                                seq: 0,
                                                delta: String::new(),
                                                done: true,
                                                error: Some(e.to_string()),
                                                prompt_tokens: 0,
                                                completion_tokens: 0,
                                            };
                                            self.send_command(chunk).await?;
                                        }
                                    }
                                }
                            }
                            _ => {
                                warn!("Received unexpected CommandV1: {:?}", cmd_v1);
                            }
                        }
                    }
                    Command::V2(_cmd_v2) => {}
                }
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
fn load_root_cert(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?; // Manually collect and handle errors

    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path);
    }
    Ok(certs)
}

#[cfg(target_os = "android")]
fn load_root_cert(path: &str) -> anyhow::Result<Vec<u8>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let certs: Vec<Vec<u8>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|cert| cert.to_vec())
        .collect();

    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path);
    }
    Ok(certs.into_iter().flatten().collect())
}

#[cfg(not(target_os = "android"))]
pub async fn create_proxy_connection(
    args: Args,
    addr: std::net::IpAddr,
    proxy_conn_id: [u8; 16],
    cert_chain_path: String,
) -> Result<()> {
    // DONE: addr is sent to server addr
    let addr_str = format!("{}:{}", addr.to_string(), args.proxy_port);
    let addr = addr_str.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid server address or port",
        )
    })?;

    let tcp_stream = match TcpStream::connect(addr).await {
        Ok(stream) => stream,
        Err(e) => {
            error!(" create proxy connection failed {}: {}", addr, e);
            return Err(e.into());
        }
    };

    match tcp_stream.set_nodelay(true) {
        Ok(_) => info!("Set nodelay for proxy connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to set nodelay for proxy connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    let cert = match load_root_cert(cert_chain_path.as_str()) {
        Ok(cert) => cert,
        Err(e) => {
            error!("Failed to load root cert: {}", e);
            return Err(e.into());
        }
    };

    let mut root_store = RootCertStore::empty();
    match root_store.add(cert[0].clone()) {
        Ok(_) => info!("Add root cert for proxy connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to add root cert for proxy connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    info!(
        " proxy_conn_id {:?} Connected to proxy port.",
        proxy_conn_id
    );

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));

    let server_addr_clone = args.server_addr.clone();
    let server_addr_clone2 = args.server_addr.clone();
    let server_name = if let Ok(ip) = server_addr_clone.parse::<std::net::IpAddr>() {
        // For IP address
        ServerName::try_from(ip.to_string())
            .map_err(|_| anyhow::anyhow!("Invalid server name: {}", server_addr_clone))?
    } else {
        // For domain name
        ServerName::try_from(args.server_addr)
            .map_err(|_| anyhow::anyhow!("Invalid server name: {}", server_addr_clone2))?
    };

    let mut tls_proxy_stream = match connector.connect(server_name, tcp_stream).await {
        Ok(stream) => stream,
        Err(e) => {
            error!("rustls: {}", e);
            return Err(anyhow!("Failed to connect to proxy port: {}", e));
        }
    };

    let notify_cmd = Command::V1(CommandV1::NewProxyConn {
        proxy_conn_id: proxy_conn_id.clone(),
    });

    match write_command(&mut tls_proxy_stream, &notify_cmd).await {
        Ok(_) => info!(
            "proxy_conn_id {:?} Sent new proxy connection notification.",
            proxy_conn_id
        ),
        Err(e) => error!("Failed to send new proxy connection notification: {}", e),
    };

    let local_stream =
        match TcpStream::connect(format!("{}:{}", args.local_addr, args.local_port)).await {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to connect to local service: {}", e);
                return Err(anyhow!("Failed to connect to local service: {}", e));
            }
        };
    info!(
        "proxy_conn_id {:?} Connected to local service at {}:{}",
        proxy_conn_id, args.local_addr, args.local_port
    );

    info!("proxy_conn_id {:?} Joining streams...", proxy_conn_id);

    match join_streams(tls_proxy_stream, local_stream).await {
        Ok(_) => {
            info!(
                "proxy_conn_id {:?} Streams joined and finished.",
                proxy_conn_id
            );
            return Ok(());
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                info!(
                    "proxy_conn_id {:?} Connection closed by peer",
                    proxy_conn_id
                );
                return Ok(());
            } else {
                error!(
                    "proxy_conn_id {:?} Error joining streams: {}",
                    proxy_conn_id, e
                );
                return Err(e.into());
            }
        }
    }
}

#[cfg(target_os = "android")]
pub async fn create_proxy_connection(
    args: Args,
    addr: std::net::IpAddr,
    proxy_conn_id: [u8; 16],
    cert_chain_path: String,
) -> Result<()> {
    // Android implementation using native TLS - simplified version
    warn!("Android TLS proxy connections are simplified - full TLS support requires additional configuration");

    // For now, just establish TCP connection without TLS
    let addr_str = format!("{}:{}", addr.to_string(), args.proxy_port);
    let addr = addr_str.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid server address or port",
        )
    })?;

    let mut tcp_stream = match TcpStream::connect(addr).await {
        Ok(stream) => stream,
        Err(e) => {
            error!("create proxy connection failed {}: {}", addr, e);
            return Err(e.into());
        }
    };

    match tcp_stream.set_nodelay(true) {
        Ok(_) => info!("Set nodelay for proxy connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to set nodelay for proxy connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    info!(
        "proxy_conn_id {:?} Connected to proxy port (Android - TCP only).",
        proxy_conn_id
    );

    let notify_cmd = Command::V1(CommandV1::NewProxyConn {
        proxy_conn_id: proxy_conn_id.clone(),
    });

    match write_command(&mut tcp_stream, &notify_cmd).await {
        Ok(_) => info!(
            "proxy_conn_id {:?} Sent new proxy connection notification.",
            proxy_conn_id
        ),
        Err(e) => error!("Failed to send new proxy connection notification: {}", e),
    };

    let local_stream =
        match TcpStream::connect(format!("{}:{}", args.local_addr, args.local_port)).await {
            Ok(stream) => stream,
            Err(e) => {
                error!("create local connection failed {}: {}", args.local_addr, e);
                return Err(e.into());
            }
        };

    match local_stream.set_nodelay(true) {
        Ok(_) => info!("Set nodelay for local connection {:?}", proxy_conn_id),
        Err(e) => error!(
            "Failed to set nodelay for local connection {:?}: {}",
            proxy_conn_id, e
        ),
    };

    info!("proxy_conn_id {:?} Connected to local port.", proxy_conn_id);

    info!("proxy_conn_id {:?} Joining streams...", proxy_conn_id);

    match join_streams(tcp_stream, local_stream).await {
        Ok(_) => {
            info!(
                "proxy_conn_id {:?} Streams joined and finished.",
                proxy_conn_id
            );
            return Ok(());
        }
        Err(e) => {
            error!(
                "proxy_conn_id {:?} Failed to join streams: {}",
                proxy_conn_id, e
            );
            return Err(e.into());
        }
    }
}
