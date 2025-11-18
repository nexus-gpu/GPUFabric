pub mod handle_api;
pub mod client;
pub mod models;

use redis::Client as RedisClient;
use sqlx::Pool; 
use sqlx::postgres::Postgres;
use anyhow::Result;
use std::sync::Arc;
use tracing::error;

#[allow(dead_code)] // API server structures and endpoints
pub struct ApiServer {
    pub db_pool: Pool<Postgres>,
    pub redis_client: Arc<RedisClient>,
}

impl ApiServer {
    pub async fn new(db_url: &str, redis_url: &str) -> Result<Self> {
        let db_pool = Pool::connect(db_url).await?;
    
        let redis_client = Arc::new(match RedisClient::open(redis_url) {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to connect to Redis: {}", e);
                return Err(anyhow::anyhow!("Redis connection failed"));
            }
        });
        Ok(ApiServer {
            db_pool,
            redis_client,
        })
    }
}

use serde::Serialize;
use chrono::{DateTime, Utc};
#[allow(dead_code)] // API response structures
#[derive(Serialize)]
pub struct ClientInfoResponse {
    pub client_id: String,
    pub authed: bool,
    system_info: Option<SystemInfoResponse>,
    pub connected_at: DateTime<Utc>,
}

#[allow(dead_code)] // API response structures
#[derive(Serialize)]
struct SystemInfoResponse {
    cpu_usage: u8,
    memory_usage: u8,
    disk_usage: u8,
    last_heartbeat: DateTime<Utc>,
    heartbeat_seconds_ago: u64,
}


