use anyhow::Result;
use clap::Parser;
use gpuf_s::{consumer, points_sync};
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value = "100")]
    pub batch_size: usize,

    #[arg(long, default_value = "5")]
    pub batch_timeout: u64,

    #[arg(
        env = "GPUF_DATABASE_URL",
        long,
        default_value = "postgres://username:password@localhost/database"
    )]
    pub database_url: String,

    #[arg(long, env = "GPUF_BOOTSTRAP_SERVER", default_value = "localhost:9092")]
    pub bootstrap_server: String,

    #[arg(long, default_value = "300")]
    pub offline_after_secs: i64,

    #[arg(long, default_value = "30")]
    pub sweep_interval_secs: u64,

    #[arg(long, default_value = "600")]
    pub points_refresh_interval_secs: u64,

    #[arg(long, env = "GPUF_POINTS_CREDIT_SYNC_ENABLED", default_value_t = false)]
    pub points_credit_sync_enabled: bool,

    #[arg(long, env = "GPUF_POINTS_CREDIT_SYNC_ENDPOINT", default_value = "")]
    pub points_credit_sync_endpoint: String,

    #[arg(
        long,
        env = "GPUF_POINTS_CREDIT_SYNC_SERVICE_TOKEN",
        default_value = ""
    )]
    pub points_credit_sync_service_token: String,

    #[arg(
        long,
        env = "GPUF_POINTS_CREDIT_SYNC_BATCH_SIZE",
        default_value = "100"
    )]
    pub points_credit_sync_batch_size: i64,

    #[arg(
        long,
        env = "GPUF_POINTS_CREDIT_SYNC_SETTLE_LAG_DAYS",
        default_value = "2"
    )]
    pub points_credit_sync_settle_lag_days: i64,

    #[arg(long, env = "GPUF_POINTS_CREDIT_SYNC_SCALE", default_value = "100")]
    pub points_credit_sync_scale: i64,

    #[arg(
        long,
        env = "GPUF_POINTS_CREDIT_SYNC_TIMEOUT_SECS",
        default_value = "10"
    )]
    pub points_credit_sync_timeout_secs: u64,

    #[arg(
        long,
        env = "GPUF_POINTS_CREDIT_SYNC_MAX_ATTEMPTS",
        default_value = "10"
    )]
    pub points_credit_sync_max_attempts: i32,
}
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("gpuf-s=info".parse()?))
        .init();

    // Parse command line arguments
    let args = Args::try_parse()?;

    // Initialize database connection pool
    let db_pool = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&args.database_url)
        .await
    {
        Ok(pool) => pool,
        Err(e) => {
            error!("Failed to connect to database: {}", e);
            return Err(anyhow::anyhow!("Database connection failed"));
        }
    };

    let sweep_pool = db_pool.clone();
    let offline_after_secs = args.offline_after_secs;
    let sweep_interval_secs = args.sweep_interval_secs;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(sweep_interval_secs));
        loop {
            ticker.tick().await;

            let res = sqlx::query(
                "UPDATE \"public\".\"gpu_assets\" \n                 SET client_status = 'offline', updated_at = NOW() \n                 WHERE valid_status = 'valid' \n                   AND client_status <> 'offline' \n                   AND updated_at < (NOW() - ($1 * INTERVAL '1 second'))",
            )
            .bind(offline_after_secs)
            .execute(&sweep_pool)
            .await;

            match res {
                Ok(r) => {
                    let n = r.rows_affected();
                    if n > 0 {
                        info!(
                            "Sweeper marked {} clients offline (offline_after_secs={})",
                            n, offline_after_secs
                        );
                    }
                }
                Err(e) => {
                    error!("Sweeper failed to mark stale clients offline: {}", e);
                }
            }
        }
    });

    let mut points_sync_config = points_sync::PointsSyncConfig {
        enabled: args.points_credit_sync_enabled,
        endpoint: args.points_credit_sync_endpoint.clone(),
        service_token: args.points_credit_sync_service_token.clone(),
        batch_size: args.points_credit_sync_batch_size,
        settle_lag_days: args.points_credit_sync_settle_lag_days,
        credit_scale: args.points_credit_sync_scale,
        request_timeout_secs: args.points_credit_sync_timeout_secs,
        max_attempts: args.points_credit_sync_max_attempts,
    };
    if let Err(e) = points_sync_config.validate() {
        warn!(error = %e, "points credit sync disabled due to invalid configuration");
        points_sync_config.enabled = false;
    }

    let points_pool = db_pool.clone();
    let points_refresh_interval_secs = args.points_refresh_interval_secs;
    let points_sync_config = points_sync_config.clone();
    tokio::spawn(async move {
        if points_refresh_interval_secs == 0 {
            info!("Points refresher disabled (points_refresh_interval_secs=0)");
            return;
        }

        let mut ticker = tokio::time::interval(Duration::from_secs(points_refresh_interval_secs));
        loop {
            ticker.tick().await;

            let res = sqlx::query("SELECT refresh_device_points_daily();")
                .execute(&points_pool)
                .await;

            match res {
                Ok(_) => {
                    info!(
                        "Refreshed device_points_daily (interval_secs={})",
                        points_refresh_interval_secs
                    );
                    if points_sync_config.enabled {
                        let sync_pool = points_pool.clone();
                        let sync_config = points_sync_config.clone();
                        tokio::spawn(async move {
                            points_sync::run_after_points_refresh(sync_pool, sync_config).await;
                        });
                    }
                }
                Err(e) => {
                    error!("Failed to refresh device_points_daily: {}", e);
                }
            }
        }
    });

    // Start the consumer service
    consumer::start_consumer_services(
        &args.bootstrap_server, // From your command line args
        "heartbeat-consumer-group",
        "client-heartbeats",
        db_pool,
        args.batch_size,    // Batch size
        args.batch_timeout, // Batch timeout in seconds
    )
    .await?;

    Ok(())
}
