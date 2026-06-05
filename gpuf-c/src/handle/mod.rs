pub mod android_sdk;
pub mod handle_tcp;
pub mod handle_udp;
pub mod handle_ws;
pub mod worker_sdk;
use crate::util::cmd::{Args, EngineType, WorkerType};
use crate::util::log_icon;
use crate::util::network_info::SessionNetworkMonitor;
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::Engine;
use common::{DevicesInfo, EngineType as ClientEngineType, OsType, SystemInfo};
use tracing::{error, info};

use anyhow::Result;
use tokio::sync::Mutex;

use futures_util::stream::{SplitSink, SplitStream};
use std::future::Future;
#[allow(unused_imports)]
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
// LLM engine is not available in lightweight Android version
#[cfg(not(target_os = "android"))]
use crate::llm_engine::AnyEngine;

use std::collections::HashSet;
use tokio::sync::Notify;

pub trait WorkerHandle: Send + Sync {
    fn login(&self) -> impl Future<Output = Result<()>> + Send;
    fn handler(&self) -> impl Future<Output = Result<()>> + Send;
    fn model_task(&self) -> impl Future<Output = Result<()>> + Send;
    fn heartbeat_task(&self) -> impl Future<Output = Result<()>> + Send;
}

pub type ControlReader = Box<dyn AsyncRead + Send + Unpin>;
pub type ControlWriter = Box<dyn AsyncWrite + Send + Unpin>;

#[cfg(not(target_os = "android"))]
pub fn install_rustls_crypto_provider_once() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    });
}

pub struct ClientWorker {
    addr: std::net::IpAddr,
    reader: Arc<Mutex<ControlReader>>,
    writer: Arc<Mutex<ControlWriter>>,
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
    #[cfg(not(target_os = "android"))]
    engine: Arc<Mutex<Option<AnyEngine>>>,
    #[cfg(target_os = "android")]
    _engine: PhantomData<()>,
}

pub type TCPWorker = ClientWorker;
pub type UDPWorker = ClientWorker;

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

    async fn model_task(&self) -> Result<()> {
        match self {
            AutoWorker::TCP(worker) => worker.model_task().await,
            AutoWorker::WS(worker) => worker.model_task().await,
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
    info!(
        "{} new_worker: Starting worker creation...",
        log_icon("🔧", "[INIT]")
    );
    // TODO: IPC shared memory should be selected
    loop {
        info!(
            "{} new_worker: Loop iteration for worker type: {:?}",
            log_icon("🔄", "[LOOP]"),
            args.worker_type
        );
        match args.worker_type {
            WorkerType::TCP => {
                info!(
                    "{} new_worker: Creating TCP worker...",
                    log_icon("📡", "[TCP]")
                );
                match TCPWorker::new(args.clone()).await {
                    Ok(worker) => {
                        info!(
                            "{} new_worker: TCP worker created successfully",
                            log_icon("✅", "[OK]")
                        );
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
                info!(
                    "{} new_worker: Creating WS worker...",
                    log_icon("🌐", "[WS]")
                );
                match WSWorker::new(args.clone()).await {
                    Ok(worker) => {
                        info!(
                            "{} new_worker: WS worker created successfully",
                            log_icon("✅", "[OK]")
                        );
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

        info!(
            "{} new_worker: Waiting 5 seconds before retry...",
            log_icon("⏳", "[WAIT]")
        );
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
