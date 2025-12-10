pub mod handle_tcp;
pub mod handle_ws;
use crate::util::cmd::{Args, WorkerType,EngineType};
use crate::util::network_info::SessionNetworkMonitor;
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::Engine;
use common::{OsType,DevicesInfo, SystemInfo, EngineType as ClientEngineType};

use anyhow::{anyhow, Result};
use std::sync::OnceLock;
use tokio::sync::Mutex;

use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use futures_util::stream::{SplitStream, SplitSink};
use std::sync::Arc;
use std::future::Future;
#[allow(unused_imports)]
use std::marker::PhantomData;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::AnyEngine;

pub trait WorkerHandle: Send + Sync {
    fn login(&self) -> impl Future<Output = Result<()>> + Send;
    fn handler(&self) -> impl Future<Output = Result<()>> + Send;
    fn model_task(&self, get_last_models: &str) -> impl Future<Output = Result<()>> + Send;
    fn heartbeat_task(&self) -> impl Future<Output = Result<()>> + Send;
}

pub struct TCPWorker {
    addr: std::net::IpAddr,
    reader: Arc<Mutex<ReadHalf<TcpStream>>>,
    writer: Arc<Mutex<WriteHalf<TcpStream>>>,
    system_info: Arc<SystemInfo>,
    devices_info: Arc<Vec<DevicesInfo>>,
    device_memtotal_gb: u32,
    device_total_tflops: u32,
    network_monitor: Arc<Mutex<SessionNetworkMonitor>>,
    client_id: [u8; 16],
    os_type: OsType,
    engine_type: ClientEngineType,
    args: Args,
    #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
    engine: Arc<Mutex<Option<AnyEngine>>>,
    #[cfg(any(target_os = "macos", target_os = "android"))]
    _engine: PhantomData<()>,
}

// WS worker
#[allow(dead_code)]

pub struct WSWorker {
    reader: Arc<Mutex<SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>>,
    writer: Arc<Mutex<SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>>>,
    args: Args,
}

pub enum AutoWorker {
    TCP(TCPWorker),
    WS(WSWorker),
}

impl WorkerHandle for AutoWorker {
    async fn login(&self) -> Result<()> {
        match self {
            AutoWorker::TCP(worker) => worker.login().await,
            AutoWorker::WS(worker) => worker.login().await,
        }
    }

    async fn handler(&self) -> Result<()> {
        match self {
            AutoWorker::TCP(worker) => worker.handler().await,
            AutoWorker::WS(worker) => worker.handler().await,
        }
    }

    async fn model_task(&self, get_last_models: &str) -> Result<()> {
        match self {
            AutoWorker::TCP(worker) => worker.model_task(get_last_models).await,
            AutoWorker::WS(worker) => worker.model_task(get_last_models).await,
        }
    }

    async fn heartbeat_task(&self) -> Result<()> {
        match self {
            AutoWorker::TCP(worker) => worker.heartbeat_task().await,
            AutoWorker::WS(worker) => worker.heartbeat_task().await,
        }
    }
}

pub async fn new_worker(args: Args) -> AutoWorker {
    // TODO: IPC shared memory should be selected
    loop {
        match args.worker_type {
            WorkerType::TCP => {
                match TCPWorker::new(args.clone()).await {
                    Ok(worker) => return AutoWorker::TCP(worker),
                    Err(e) => {
                        tracing::error!("Failed to create TCP worker: {}. Retrying in 5 seconds...", e);
                    }
                }
            }
            WorkerType::WS => {
                match WSWorker::new(args.clone()).await {
                    Ok(worker) => return AutoWorker::WS(worker),
                    Err(e) => {
                        tracing::error!("Failed to create WS worker: {}. Retrying in 5 seconds...", e);
                    }
                }
            }
        }
        
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

// ============================================================================
// Android JNI Integration - Global Worker Management
// ============================================================================

/// Global worker instance for Android JNI
static GLOBAL_WORKER: OnceLock<Mutex<Option<Arc<AutoWorker>>>> = OnceLock::new();

/// Global worker task handle for background operations
static GLOBAL_WORKER_HANDLES: OnceLock<Mutex<Option<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)>>> = OnceLock::new();

/// Initialize global worker for Android
#[cfg(target_os = "android")]
pub async fn init_global_worker(args: Args) -> Result<()> {
    // Create new worker
    let worker = new_worker(args).await;
    
    // Login to server
    worker.login().await
        .map_err(|e| anyhow!("Failed to login worker: {}", e))?;
    
    // Wrap in Arc for shared access
    let worker_arc = Arc::new(worker);
    
    // Store in global instance
    let global = GLOBAL_WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = global.lock().await;
    *guard = Some(worker_arc);
    
    tracing::info!("Global worker initialized successfully");
    Ok(())
}

/// Start background worker tasks (heartbeat, handler, etc.)
#[cfg(target_os = "android")]
pub async fn start_worker_tasks() -> Result<()> {
    let global = GLOBAL_WORKER.get()
        .ok_or_else(|| anyhow!("Worker not initialized"))?;
    
    // Get Arc<AutoWorker> for shared access
    let worker_arc = {
        let guard = global.lock().await;
        guard.as_ref()
            .ok_or_else(|| anyhow!("Worker not available"))?
            .clone()
    };
    
    // Spawn heartbeat task
    let heartbeat_worker = worker_arc.clone();
    let heartbeat_handle = tokio::spawn(async move {
        if let Err(e) = heartbeat_worker.heartbeat_task().await {
            tracing::error!("Heartbeat task failed: {}", e);
        }
    });
    
    // Spawn handler task
    let handler_worker = worker_arc.clone();
    let handler_handle = tokio::spawn(async move {
        if let Err(e) = handler_worker.handler().await {
            tracing::error!("Handler task failed: {}", e);
        }
    });
    
    // Store handles separately for proper cleanup
    let global_handles = GLOBAL_WORKER_HANDLES.get_or_init(|| Mutex::new(None));
    let mut guard = global_handles.lock().await;
    *guard = Some((heartbeat_handle, handler_handle));
    
    tracing::info!("Worker tasks started successfully (concurrent)");
    Ok(())
}

/// Stop global worker and cleanup
#[cfg(target_os = "android")]
pub async fn stop_global_worker() {
    // Stop background tasks
    if let Some(global_handles) = GLOBAL_WORKER_HANDLES.get() {
        let mut guard = global_handles.lock().await;
        if let Some((heartbeat_handle, handler_handle)) = guard.take() {
            heartbeat_handle.abort();
            handler_handle.abort();
            tracing::info!("Worker tasks stopped");
        }
    }
    
    // Cleanup worker
    if let Some(global) = GLOBAL_WORKER.get() {
        let mut guard = global.lock().await;
        *guard = None;
        tracing::info!("Global worker cleaned up");
    }
}

/// Get global worker status
#[cfg(target_os = "android")]
pub async fn get_worker_status() -> Result<String> {
    let global = GLOBAL_WORKER.get()
        .ok_or_else(|| anyhow!("Worker not initialized"))?;
    
    let guard = global.lock().await;
    if guard.is_some() {
        Ok("Worker is running".to_string())
    } else {
        Ok("Worker not available".to_string())
    }
}
