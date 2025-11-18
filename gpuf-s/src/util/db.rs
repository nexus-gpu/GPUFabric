use anyhow::Result;
use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;
use redis::{Client};
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use tracing::{error, info};

// Database functions
pub async fn init_db(
    bootstrap_server: &str,
    database_url: &str,
    redis_url: &str,
) -> Result<(Arc<Pool<Postgres>>, Arc<Client>, Arc<FutureProducer>)> {
    let db_pool = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
    {
        Ok(pool) => pool,
        Err(e) => {
            error!("Failed to connect to database: {}", e);
            return Err(anyhow::anyhow!("Database connection failed"));
        }
    };
    info!("Connected to database successfully");
    // Initialize Redis client
    let redis_client = Arc::new(match Client::open(redis_url) {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to connect to Redis: {}", e);
            return Err(anyhow::anyhow!("Redis connection failed"));
        }
    });
    info!("Connected to Redis successfully");

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_server)
        // #[cfg(debug_assertions)]
        // .set("debug", "all")
        .create()
        .expect("Producer creation error");

    Ok((Arc::new(db_pool), redis_client, Arc::new(producer)))
}


