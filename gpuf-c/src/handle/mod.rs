pub mod android_sdk;
pub mod handle_tcp;
pub mod handle_ws;
use crate::util::cmd::{Args, EngineType, WorkerType};
use crate::util::network_info::SessionNetworkMonitor;
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::Engine;
use common::{DevicesInfo, EngineType as ClientEngineType, OsType, SystemInfo};
use tracing::{error, info};

use anyhow::{anyhow, Result};
use tokio::sync::Mutex;

use futures_util::stream::{SplitSink, SplitStream};
use std::future::Future;
#[allow(unused_imports)]
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::AnyEngine;

use std::collections::HashSet;
use tokio::sync::Notify;

pub trait WorkerHandle: Send + Sync {
    fn login(&self) -> impl Future<Output = Result<()>> + Send;
    fn handler(&self) -> impl Future<Output = Result<()>> + Send;
    fn model_task(&self, get_last_models: &str) -> impl Future<Output = Result<()>> + Send;
    fn heartbeat_task(&self) -> impl Future<Output = Result<()>> + Send;
}

pub struct ClientWorker {
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
    cancel_state: Arc<CancelState>,
    #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
    engine: Arc<Mutex<Option<AnyEngine>>>,
    #[cfg(any(target_os = "macos", target_os = "android"))]
    _engine: PhantomData<()>,
}

pub type TCPWorker = ClientWorker;

pub struct CancelState {
    pub cancelled: Mutex<HashSet<String>>,
    pub notify: Notify,
}

// WS worker
#[allow(dead_code)]

pub struct WSWorker {
    reader: Arc<
        Mutex<
            SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
        >,
    >,
    writer: Arc<
        Mutex<
            SplitSink<
                WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
                Message,
            >,
        >,
    >,
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
    info!("🔧 new_worker: Starting worker creation...");
    // TODO: IPC shared memory should be selected
    loop {
        info!(
            "🔄 new_worker: Loop iteration for worker type: {:?}",
            args.worker_type
        );
        match args.worker_type {
            WorkerType::TCP => {
                info!("📡 new_worker: Creating TCP worker...");
                match TCPWorker::new(args.clone()).await {
                    Ok(worker) => {
                        info!("✅ new_worker: TCP worker created successfully");
                        return AutoWorker::TCP(worker);
                    }
                    Err(e) => {
                        error!(
                            "Failed to create TCP worker: {}. Retrying in 5 seconds...",
                            e
                        );
                    }
                }
            }
            WorkerType::WS => {
                info!("🌐 new_worker: Creating WS worker...");
                match WSWorker::new(args.clone()).await {
                    Ok(worker) => {
                        info!("✅ new_worker: WS worker created successfully");
                        return AutoWorker::WS(worker);
                    }
                    Err(e) => {
                        error!(
                            "Failed to create WS worker: {}. Retrying in 5 seconds...",
                            e
                        );
                    }
                }
            }
        }

        info!("⏳ new_worker: Waiting 5 seconds before retry...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
