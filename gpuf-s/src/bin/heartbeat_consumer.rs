use anyhow::Result;
use clap::Parser;
use gpuf_s::consumer;
use tracing::error;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value = "100")]
    pub batch_size: usize,

    #[arg(long, default_value = "5")]
    pub batch_timeout: u64,

    #[arg(
        long,
        default_value = "postgres://username:password@localhost/database"
    )]
    pub database_url: String,

    #[arg(long, default_value = "localhost:9092")]
    pub bootstrap_server: String,
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
