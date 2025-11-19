pub mod handle_tcp;
pub mod handle_ws;
use crate::util::cmd::{Args, WorkerType,EngineType};
use crate::util::network_info::SessionNetworkMonitor;
use crate::llm_engine::Engine;
use common::{OsType,DevicesInfo, SystemInfo, EngineType as ClientEngineType};

use anyhow::{anyhow, Result};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use futures_util::stream::{SplitStream, SplitSink};
use std::sync::Arc;
use std::future::Future;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
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
    engine: Arc<Mutex<Option<AnyEngine>>>,
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
    match args.worker_type {
        WorkerType::TCP => AutoWorker::TCP(TCPWorker::new(args).await.expect("Failed to create TCP worker")),
        WorkerType::WS => AutoWorker::WS(WSWorker::new(args).await.expect("Failed to create WS worker")),
    }
}
