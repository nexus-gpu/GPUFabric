pub mod handle_connections;
pub mod handle_agent;

use crate::db::{models::{HotModelClass}, models::ClientModelClass};
use crate::util::{
    protoc::{ClientId, ProxyConnId}, cmd, db,
};
use crate::inference::InferenceScheduler;
use crate::util::pack::BufferPool;

use redis::Client as RedisClient;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres};
use chrono::{DateTime, Utc};
use common::{join_streams, read_command, write_command, Command, CommandV1, DevicesInfo, Model};
use std::collections::HashMap;
use std::sync::Arc;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::Producer;
use anyhow::{anyhow, Result};
use bytes::BytesMut;
use tokio::net::{TcpStream,tcp::OwnedWriteHalf};
use tokio::sync::Mutex;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tracing::{error,info};

pub type UserDb = Arc<Mutex<HashMap<String, User>>>;
pub type TokenDb = Arc<Mutex<HashMap<String, String>>>;
pub type ActiveClients = Arc<Mutex<HashMap<ClientId, ClientInfo>>>;
pub type PendingConnections = Arc<Mutex<HashMap<ProxyConnId, (TcpStream,BytesMut)>>>;

pub struct ClientInfo {
    pub writer: Arc<Mutex<OwnedWriteHalf>>,
    pub authed: bool,
    #[allow(dead_code)] // Client protocol version
    pub version: u32,
    pub system_info: Option<SystemInfo>,
    #[allow(dead_code)] // Connected devices information
    pub devices_info: Vec<DevicesInfo>,
    #[allow(dead_code)] // Connection timestamp
    pub connected_at: DateTime<Utc>,
    pub models: Option<Vec<Model>>,
}

pub struct User {
    #[allow(dead_code)] // User password hash
    pub pass: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub cpu_usage: u8,
    pub memory_usage: u8,
    pub disk_usage: u8,
    pub device_memsize: u32,
    pub total_tflops: u32,
    pub last_heartbeat: std::time::SystemTime,
    pub memsize_gb: u32,
}


#[allow(dead_code)]
#[derive(Serialize)]
struct ServerStats {
    active_clients: usize,
    pending_connections: usize,
    total_connections: u64,
    uptime_seconds: u64,
}

#[derive(Serialize, Clone)]
pub struct ServerConfig {
    pub control_port: u16,
    pub proxy_port: u16,
    pub public_port: u16,
    pub api_port: u16,
}


#[derive(Clone)]
pub struct ServerState {
    pub active_clients: ActiveClients,
    pub pending_connections: PendingConnections,
    #[allow(dead_code)] // User authentication database
    pub user_db: UserDb,
    #[allow(dead_code)] // Token authentication database
    pub token_db: TokenDb,
    #[allow(dead_code)] // Server start timestamp
    pub server_start_time: DateTime<Utc>,
    pub total_connections: Arc<Mutex<u64>>,
    #[allow(dead_code)] // Server configuration
    pub config: ServerConfig,
    #[allow(dead_code)] // Database pool
    pub db_pool: Arc<Pool<Postgres>>,
    #[allow(dead_code)] // Redis client
    pub redis_client: Arc<RedisClient>,
    #[allow(dead_code)] // Kafka producer
    pub producer: Arc<FutureProducer>,
    pub inference_scheduler: Arc<InferenceScheduler>,
    pub client_model: Arc<ClientModelClass>,
    pub hot_models: Arc<HotModelClass>,
    pub cert_chain: Arc<Vec<CertificateDer<'static>>>,
    pub priv_key: Arc<PrivateKeyDer<'static>>,
    pub buffer_pool: Arc<BufferPool>,
}

impl Drop for ServerState {
    fn drop(&mut self) {
        if let Err(e) = self.producer.flush(std::time::Duration::from_secs(1)) {
            error!("Failed to flush Kafka producer: {:?}", e);
        }
        info!("ServerState is being dropped, resources cleaned up");
    }
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatCompletionRequest {
    model: String,
}

pub async fn new_server_state(args: &cmd::Args) -> Result<ServerState, anyhow::Error> {
    // check cert chain path
    let cert_chain_path = args.proxy_cert_chain_path.clone();
    if std::path::Path::new(&cert_chain_path).exists() {
        info!(" Certificate chain path exists: {}", cert_chain_path);
    } else {
        return Err(anyhow!(
            " Certificate chain path does not exist: {}",
            cert_chain_path
        ));
    }

    // check private key path
    let private_key_path = args.proxy_private_key_path.clone();
    if std::path::Path::new(&private_key_path).exists() {
        info!(" Private key path exists: {}", private_key_path);
    } else {
        return Err(anyhow!(
            " Private key path does not exist: {}",
            private_key_path
        ));
    }

    let (db_pool, redis_client, producer): (Arc<Pool<Postgres>>, Arc<RedisClient>, Arc<FutureProducer>) = db::init_db(&args.bootstrap_server, &args.database_url, &args.redis_url).await?;

    let active_clients = Arc::new(Mutex::new(HashMap::new()));
    let pending_connections = Arc::new(Mutex::new(HashMap::new()));
    let user_db = Arc::new(Mutex::new(HashMap::<String, User>::new()));
    let token_db = Arc::new(Mutex::new(HashMap::new()));
    let total_connections = Arc::new(Mutex::new(0u64));
    let server_start_time = Utc::now();
    let cert_chain = crate::util::load_certs(&args.proxy_cert_chain_path)?;
    let priv_key = crate::util::load_private_key(&args.proxy_private_key_path)?;
    
    // Initialize inference scheduler
    let inference_scheduler = Arc::new(InferenceScheduler::new(active_clients.clone()));
    
    let app_state = ServerState {
        active_clients: active_clients.clone(),
        pending_connections: pending_connections.clone(),
        user_db: user_db.clone(),
        token_db: token_db.clone(),
        server_start_time,
        total_connections: total_connections.clone(),
        config: ServerConfig {
            control_port: args.control_port,
            proxy_port: args.proxy_port,
            public_port: args.public_port,
            api_port: args.api_port,
        },
        buffer_pool: Arc::new(BufferPool::new(8 * 1024, 16)),
        db_pool: db_pool.clone(),
        redis_client: redis_client.clone(),
        producer: producer.clone(),
        cert_chain: cert_chain.into(),
        priv_key: Arc::new(priv_key),
        hot_models: Arc::new(HotModelClass::new(db_pool.clone())),
        client_model: Arc::new(ClientModelClass::new(db_pool.clone())),
        inference_scheduler,
    };
    // If monitor flag is set, just print monitoring data and exit
    if args.monitor {
        print_monitoring_data(active_clients.clone()).await;
    }
    Ok(app_state)
}

pub async fn print_monitoring_data(active_clients: ActiveClients) {
    let clients = active_clients.lock().await;
    if clients.is_empty() {
        println!("No active clients.");
        return;
    }

    println!("Client Monitoring Data:");
    println!(
        "{:<20} {:<10} {:<10} {:<10} {:<20}",
        "Client ID", "CPU (%)", "Memory (%)", "Disk (%)", "Last Heartbeat"
    );
    println!("{}", "-".repeat(80));

    for (client_id, client_info) in clients.iter() {
        if let Some(sys_info) = &client_info.system_info {
            let duration = sys_info
                .last_heartbeat
                .elapsed()
                .unwrap_or(std::time::Duration::from_secs(0));
            let seconds = duration.as_secs();
            println!(
                "{:<20} {:<10.2} {:<10.2} {:<10.2} {:<20}",
                client_id,
                sys_info.cpu_usage,
                sys_info.memory_usage,
                sys_info.disk_usage,
                format!("{}s ago", seconds)
            );
        } else {
            println!(
                "{:<20} {:<10} {:<10} {:<10} {:<20}",
                client_id, "N/A", "N/A", "N/A", "No data"
            );
        }
    }
}




// Response structure
#[derive(Debug, Serialize)]
#[allow(dead_code)] // Client statistics response structure
pub struct ClientStatResponse {
    pub systems_total_number: i64,
    pub systems_online_number: i64,
    pub systems_maintenance_number: i64,
    pub systems_warnings_number: i64,
    pub total_tflops: i64,
    pub uptime_rate: i32,
}
