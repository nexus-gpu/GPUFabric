use super::*;
use crate::util::{
    system_info::{
        collect_device_info, collect_system_info, get_engine_models, pull_ollama_model, run_model,
    },
};
#[cfg(not(target_os = "macos"))]
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::{self, llama_engine::LlamaEngine};
use anyhow::Result;
use common::{
    format_bytes, format_duration, join_streams, read_command, write_command, Command,
    CommandV1, EngineType as ClientEngineType, SystemInfo, MAX_MESSAGE_SIZE, OsType,
};

use bytes::BytesMut;
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

const CURRENT_VERSION: u32 = 1;

impl TCPWorker {
    pub async fn new(args: Args) -> Result<Self> {
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
        let (device_info, device_memtotal_mb) = match collect_device_info().await {
            Ok(info) => {
                info
            },
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
                        4096,  // context size
                        999,    // GPU layers (999 = try to offload all layers)
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
                    },
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
                info!("Starting LLAMA HTTP API server on {}:{}", local_addr, local_port);
                
                use std::sync::Arc;
                use tokio::sync::RwLock;
                use crate::llm_engine::llama_server::start_server;
                
                // Wrap the shared engine in Arc<RwLock> for HTTP server
                let engine_arc = Arc::new(RwLock::new(server_engine));
                
                // Spawn server in background
                tokio::spawn(async move {
                    if let Err(e) = start_server(engine_arc, &local_addr_clone, local_port).await {
                        error!("LLAMA HTTP server error: {}", e);
                    }
                });
                
                info!("LLAMA HTTP API server started successfully on {}:{}", local_addr, local_port);
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
            write_command(&mut *self.writer.lock().await, &Command::V1(login_cmd)).await?;
            Ok(())
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

                    let models = match get_engine_models(local_port).await {
                        Ok(models) => {
                            info!("Successfully fetched {} models from Ollama.", models.len());
                            Some(models)
                        }
                        Err(e) => {
                            warn!("Could not fetch models from Ollama: {}. This is okay if Ollama is not running.", e);
                            None
                        }
                    };
                    let model_cmd = CommandV1::ModelStatus {
                        client_id: *client_id,
                        models: models.unwrap_or_default(),
                        auto_models_device: devices_info.clone().to_vec(),
                    };
                    let _ =
                        write_command(&mut *writer_clone.lock().await, &Command::V1(model_cmd)).await;
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
                    let mut writer = { writer_clone.lock().await };
                    info!("Sending heartbeat to server cpu_usage {}% memory_usage {}% disk_usage {}% device_memtotal {}mb", cpu_usage, memory_usage, disk_usage, device_memtotal_mb);
                    let mut network_monitor = network_monitor.lock().await;
                    let stats = network_monitor.refresh().unwrap_or((0, 0));
                    let session_stats = network_monitor.get_session_stats();
                    info!(
                        "Network stats - Current: up {} down {} | Session Total: up {} down {} | Duration: {} ", 
                        format_bytes!(stats.1),
                        format_bytes!(stats.0),
                        format_bytes!(session_stats.1),
                        format_bytes!(session_stats.0),
                        format_duration!(session_stats.2.as_secs())
                    );
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
                    Command::V1(CommandV1::LoginResult {
                        success,
                        pods_model,
                        error,
                    }) => {
                        if success {
                            if pods_model.is_empty() {
                                error!("Received empty models from server");
                                return Err(anyhow!("device is not compatible with the model"));
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
                    Command::V1(CommandV1::PullModelResult { pods_model, error }) => {
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
                                        pull_ollama_model(&model_name, self.args.local_port).await?
                                    }
                                    common::EngineType::Vllm => {
                                        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
                                        if let Some(_engine) = self.engine.lock().await.as_mut() {
                                            // Engine functionality disabled in lightweight version
                                        }
                                    }
                                    _ => {}
                                }
                                match run_model(self.args.local_port, &model_name, "hello world").await
                                {
                                    Ok(output) => info!("Model {} output: {}", model_name, output),
                                    Err(e) => error!("run_model Error: {}", e),
                                }
                            }
                        }
                    }
                    Command::V1(CommandV1::RequestNewProxyConn { proxy_conn_id }) => {
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
                    _ => {
                        warn!("Received unexpected command");
                    }
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

    info!("proxy_conn_id {:?} Connected to proxy port (Android - TCP only).", proxy_conn_id);

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
            error!("proxy_conn_id {:?} Failed to join streams: {}", proxy_conn_id, e);
            return Err(e.into());
        }
    }
}
