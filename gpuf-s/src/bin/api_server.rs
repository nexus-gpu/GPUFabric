use anyhow::Result;
use clap::Parser;
use gpuf_s::api_server::ApiServer;
use std::sync::Arc;
use tracing::Level;

#[derive(Parser, Debug)]
#[command(author, version, about = "gpuf-s API server")]
struct Args {
    #[arg(short, long, default_value_t = 18081)]
    port: u16,

    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    #[arg(long, default_value = "redis://localhost:6379", env = "REDIS_URL")]
    redis_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    let args = Args::parse();

    let server_state = Arc::new(ApiServer::new(&args.database_url, &args.redis_url).await?);

    server_state.run_api_server(args.port).await?;
    Ok(())
}
