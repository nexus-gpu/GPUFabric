use anyhow::Result;
use clap::Parser;
use gpuf_c::{init, create_worker, config::Args, WorkerHandle};

#[tokio::main]
async fn main() -> Result<()> {
    init()?;

    let args = Args::parse().load_config()?;

    let worker = create_worker(args).await?;

    worker.login().await?;
    worker.handler().await?;
    Ok(())
}
