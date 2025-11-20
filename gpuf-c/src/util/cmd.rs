use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};

use crate::util::config::{ Config};
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short('f'), long)]
    pub config: Option<String>,

    /// Unique ID for this client instance. If not provided, uses machine ID.
    #[arg(short('i'), long, value_parser = parse_client_id, required_unless_present = "config")]
    pub client_id: Option<[u8; 16]>,

    /// Address of the gpuf-s server.
    #[arg(short, long, default_value = "127.0.0.1")]
    pub server_addr: String,

    /// Port for the gpuf-s control connection.
    #[arg(long, default_value_t = 17000)]
    pub control_port: u16,

    /// Port for the gpuf-s proxy connection.
    #[arg(long, default_value_t = 17001)]
    pub proxy_port: u16,

    /// Address of the local service to expose.
    #[arg(long, default_value = "127.0.0.1")]
    pub local_addr: String,

    /// Port of the local service to expose.
    #[arg(long, default_value_t = 11434)]
    pub local_port: u16,

    /// Certificate chain for TLS
    #[arg(long, default_value = "ca-cert.pem")]
    pub cert_chain_path: String,

    #[arg(
        long,
        default_value = "tcp",
        help = "type of worker to use (tcp or ws)"
    )]
    pub worker_type: WorkerType,

    #[arg(
        long,
        default_value = "ollama",
        help = "type of engine to use (vllm or ollama)"
    )]
    pub engine_type: EngineType,

    #[arg(long, default_value = "false", help = "auto mode")]
    pub auto_models: bool,

    #[arg(long, default_value = None, help = "hugging face hub token" )]
    pub hugging_face_hub_token: Option<String>,

    #[arg(long, default_value = None, help = "chat template path" )]
    pub chat_template_path: Option<String>,

    /// Run as standalone LLAMA API server (no GPUFabric connection)
    #[arg(long, help = "Run as standalone LLAMA API server")]
    pub standalone_llama: bool,

    /// Model path for standalone LLAMA server
    #[arg(long, help = "Path to GGUF model file for standalone mode")]
    pub llama_model_path: Option<String>,
}

impl Args {
    pub fn load_config(&self) -> Result<Args> {
        if let Some(config_path) = &self.config {
            // Try to load from config file
            let config_data = Config::from_file(config_path)
                .with_context(|| format!("Failed to load config from {}", config_path))?;
            // Parse worker_type from string to WorkerType enum
            let worker_type = match config_data.client.worker_type.to_lowercase().as_str() {
                "tcp" => WorkerType::TCP,
                "ws" => WorkerType::WS,
                _ => {
                    return Err(anyhow::anyhow!(
                        "Invalid worker_type in config. Must be 'tcp' or 'ws'"
                    ))
                }
            };

            // Parse engine_type from string to EngineType enum
            let engine_type = match config_data.client.engine_type.to_lowercase().as_str() {
                "vllm" => EngineType::VLLM,
                "ollama" => EngineType::OLLAMA,
                "llama" => EngineType::LLAMA,
                _ => {
                    return Err(anyhow::anyhow!(
                        "Invalid engine_type in config. Must be 'vllm' or 'ollama'"
                    ))
                }
            };

            // Parse client_id from string to [u8; 16]
            let client_id = parse_client_id(&config_data.client.client_id)
                .map_err(|e| anyhow::anyhow!("Invalid client_id format in config: {}", e))?;
            
            info!("client_id: {:?}", client_id);
            
            Ok(Args {
                config: Some(config_path.clone()),
                client_id: Some(client_id),
                server_addr: config_data.server.addr,
                control_port: config_data.server.control_port,
                proxy_port: config_data.server.proxy_port,
                local_addr: config_data.client.local_addr,
                local_port: config_data.client.local_port,
                cert_chain_path: config_data.client.cert_chain_path,
                worker_type: worker_type,
                engine_type: engine_type,
                auto_models: config_data.client.auto_models,
                hugging_face_hub_token: config_data.client.hugging_face_hub_token,
                chat_template_path: config_data.client.chat_template_path,
                standalone_llama: false,  // Config file doesn't support standalone mode
                llama_model_path: None,
            })
            
        } else {
            if self.client_id.is_none() {
                return Err(anyhow::anyhow!(
                    "Either --config or --client-id must be provided"
                ));
            }

            Ok(self.clone())
        }
    }
}

fn parse_client_id(s: &str) -> Result<[u8; 16], String> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(|e| format!("Invalid hex string: {}", e))?;
    Ok(bytes
        .try_into()
        .map_err(|_| format!("Invalid client ID length"))?)
}

#[derive(ValueEnum, Debug, Clone)]
pub enum WorkerType {
    #[clap(name = "tcp")]
    TCP,
    #[clap(name = "ws")]
    WS,
}

#[derive(ValueEnum, Debug, Clone, PartialEq)]
pub enum EngineType {
    #[clap(name = "vllm")]
    VLLM,
    #[clap(name = "ollama")]
    OLLAMA,
    #[clap(name = "llama")]
    LLAMA,
}
