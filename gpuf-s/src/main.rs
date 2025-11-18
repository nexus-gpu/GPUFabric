mod consumer;
mod api_server;
mod db;
mod handle;
mod util;
#[cfg(all(feature = "xdp", target_os = "linux"))]
mod xdp;

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::net::TcpListener;
#[cfg(target_os = "linux")]
use tokio::signal::unix::{signal, SignalKind};
use tracing::{info, Level};

#[cfg(debug_assertions)]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::main]
async fn main() -> Result<()> {
    //track memory usage in debug mode
    #[cfg(debug_assertions)]
    let profiler = dhat::Profiler::new_heap();

    //parse args
    let args = util::cmd::Args::parse();
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    //bind port
    let control_listener = TcpListener::bind(format!("0.0.0.0:{}", args.control_port)).await?;
    let proxy_listener = TcpListener::bind(format!("0.0.0.0:{}", args.proxy_port)).await?;
    let public_listener = TcpListener::bind(format!("0.0.0.0:{}", args.public_port)).await?;
    info!(
        "GPFS listening on ports: Control={}, Proxy={}, Public={}, API={}",
        args.control_port, args.proxy_port, args.public_port, args.api_port
    );
    // Create a channel to signal when to drop ServerState
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn a task to handle signals
    let server_state = Arc::new(handle::new_server_state(&args).await?);
    let server_state1 = Arc::clone(&server_state);
    let server_state2 = Arc::clone(&server_state);
    let server_state3 = Arc::clone(&server_state);

    tokio::spawn(async move {
        #[cfg(target_os = "linux")]
        {
            let mut sigterm = signal(SignalKind::terminate()).expect("Failed to create SIGTERM listener");
            let mut sigint = signal(SignalKind::interrupt()).expect("Failed to create SIGINT listener");
            
            tokio::select! {
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, shutting down gracefully...");
                }
                _ = sigint.recv() => {
                    info!("Received SIGINT, shutting down gracefully...");
                }
            }
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            // On Windows, we'll use Ctrl-C handling through tokio's default signal handling
            info!("Running on Windows - signal handling through default mechanisms");
        }
        
        // Send shutdown signal
        let _ = shutdown_tx.send(());
    });
    //init server state
    let server_loop = async {
        tokio::select! {
            res = server_state1.handle_client_connections(control_listener) => res,
            res = server_state2.handle_proxy_connections(proxy_listener) => res,
            res = server_state3.handle_public_connections(public_listener) => res,
            _ = &mut shutdown_rx => {
                info!("Shutdown signal received, stopping server...");
                Ok(())
            }
        }
    };

    let result = server_loop.await;
    info!("Dropping ServerState...");
    drop(server_state);

    #[cfg(debug_assertions)]
    drop(profiler);
    result
}
