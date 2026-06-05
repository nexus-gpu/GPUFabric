use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};

use crate::util::config::Config;
use tracing::info;

#[derive(ValueEnum, Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum LlamaSplitModeArg {
    #[clap(name = "none")]
    None,
    #[clap(name = "layer")]
    Layer,
    #[clap(name = "row")]
    Row,
}

impl std::str::FromStr for LlamaSplitModeArg {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "none" => Ok(Self::None),
            "layer" => Ok(Self::Layer),
            "row" => Ok(Self::Row),
            other => Err(format!(
                "Invalid llama_split_mode '{}'. Must be one of: none, layer, row",
                other
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub require_api_key: bool,
    pub enforce_model_checksum: bool,
    pub allow_public_listen: bool,
    pub enforce_p2p_hmac: bool,
    pub safe_download_path: bool,
    pub allow_external_model_path: bool,
}

impl SecurityConfig {
    pub fn from_args(args: &Args) -> Self {
        Self {
            require_api_key: args.api_key.is_some(),
            enforce_model_checksum: true,
            allow_public_listen: args.p2p_public_listen,
            enforce_p2p_hmac: true,
            safe_download_path: true,
            allow_external_model_path: false,
        }
    }

    pub fn emit_high_risk_warnings(&self) {
        if self.allow_public_listen {
            tracing::warn!("SECURITY: public P2P listen is explicitly enabled");
        }
        if !self.enforce_p2p_hmac {
            tracing::warn!("SECURITY: P2P data-plane HMAC enforcement is disabled");
        }
        if !self.enforce_model_checksum {
            tracing::warn!("SECURITY: model checksum enforcement is disabled");
        }
        if !self.safe_download_path {
            tracing::warn!("SECURITY: safe download path enforcement is disabled");
        }
        if self.allow_external_model_path {
            tracing::warn!("SECURITY: external model paths are allowed");
        }
    }
}

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short('f'), long)]
    pub config: Option<String>,

    /// Unique ID for this client instance. If not provided, uses machine ID.
    #[arg(short('i'), long, value_parser = parse_client_id, required_unless_present_any = ["config", "standalone_llama"])]
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

    /// IP address to advertise to peers for P2P direct connections (host candidate).
    /// If not set, gpuf-c will try to auto-detect an outbound IP.
    #[arg(long, default_value = None)]
    pub p2p_advertise_ip: Option<String>,

    /// UDP port used for P2P data-plane when running in UDP mode.
    #[arg(long, default_value_t = 40000)]
    pub p2p_udp_port: u16,

    /// Address to bind for P2P UDP data-plane. Defaults to loopback.
    #[arg(long, default_value = "127.0.0.1")]
    pub p2p_bind_addr: String,

    /// Explicitly allow P2P UDP data-plane to bind to a non-loopback address.
    #[arg(long, default_value_t = false)]
    pub p2p_public_listen: bool,

    /// Certificate chain for TLS
    #[arg(long, default_value = "ca-cert.pem")]
    pub cert_chain_path: String,

    /// Connect to the gpuf-s control port over TLS. Disabled by default for compatibility.
    #[arg(long, default_value_t = false)]
    pub control_tls: bool,

    /// Optional SNI/server name override for --control-tls certificate validation.
    #[arg(long, default_value = None)]
    pub control_tls_server_name: Option<String>,

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

    /// API key for standalone OpenAI/Anthropic-compatible routes (or GPUF_API_KEY).
    #[arg(long, env = "GPUF_API_KEY", default_value = None)]
    pub api_key: Option<String>,

    /// Model path for standalone LLAMA server
    #[arg(long, help = "Path to GGUF model file for standalone mode")]
    pub llama_model_path: Option<String>,

    /// Number of GPU layers to offload (default: 99 for large models)
    #[arg(
        long,
        default_value_t = 99,
        help = "Number of model layers to offload to GPU"
    )]
    pub n_gpu_layers: u32,

    /// Context size for model inference (default: 8192)
    #[arg(long, default_value_t = 8192, help = "Context window size in tokens")]
    pub n_ctx: u32,

    /// Batch size for prompt processing (default: 4096)
    #[arg(
        long,
        default_value_t = 4096,
        help = "Batch size for prompt processing"
    )]
    pub n_batch: u32,

    #[arg(
        long,
        default_value = "layer",
        help = "Llama multi-GPU split mode: none, layer, row"
    )]
    pub llama_split_mode: LlamaSplitModeArg,

    #[arg(
        long,
        default_value_t = 0,
        help = "Main GPU index for llama.cpp (scratch/small tensors)"
    )]
    pub llama_main_gpu: i32,

    #[arg(
        long,
        default_value = None,
        help = "Comma-separated ggml backend device indices to use (e.g. '0,1'); empty uses default"
    )]
    pub llama_devices: Option<String>,

    #[arg(
        long,
        default_value_t = 1,
        help = "Max bytes per streamed delta chunk sent to server"
    )]
    pub stream_chunk_bytes: usize,
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

            let llama_split_mode = match config_data
                .client
                .llama_split_mode
                .as_deref()
                .map(|s| s.parse::<LlamaSplitModeArg>())
            {
                Some(Ok(v)) => v,
                Some(Err(e)) => return Err(anyhow::anyhow!(e)),
                None => self.llama_split_mode.clone(),
            };

            Ok(Args {
                config: Some(config_path.clone()),
                client_id: Some(client_id),
                server_addr: config_data.server.addr,
                control_port: config_data.server.control_port,
                proxy_port: config_data.server.proxy_port,
                local_addr: config_data.client.local_addr,
                local_port: config_data.client.local_port,
                p2p_advertise_ip: self.p2p_advertise_ip.clone(),
                p2p_udp_port: self.p2p_udp_port,
                p2p_bind_addr: self.p2p_bind_addr.clone(),
                p2p_public_listen: self.p2p_public_listen,
                cert_chain_path: config_data.client.cert_chain_path,
                control_tls: config_data.client.control_tls.unwrap_or(self.control_tls),
                control_tls_server_name: config_data
                    .client
                    .control_tls_server_name
                    .clone()
                    .or_else(|| self.control_tls_server_name.clone()),
                worker_type: worker_type,
                engine_type: engine_type,
                auto_models: config_data.client.auto_models,
                hugging_face_hub_token: config_data.client.hugging_face_hub_token,
                chat_template_path: config_data.client.chat_template_path,
                standalone_llama: false, // Config file doesn't support standalone mode
                api_key: self.api_key.clone(),
                llama_model_path: None,
                n_ctx: config_data.client.n_ctx,
                n_batch: self.n_batch,
                n_gpu_layers: config_data.client.n_gpu_layers,
                llama_split_mode,
                llama_main_gpu: config_data
                    .client
                    .llama_main_gpu
                    .unwrap_or(self.llama_main_gpu),
                llama_devices: config_data
                    .client
                    .llama_devices
                    .clone()
                    .or_else(|| self.llama_devices.clone()),
                stream_chunk_bytes: self.stream_chunk_bytes,
            })
        } else {
            // In standalone_llama mode, client_id is optional
            if self.client_id.is_none() && !self.standalone_llama {
                return Err(anyhow::anyhow!(
                    "Either --config, --client-id, or --standalone-llama must be provided"
                ));
            }

            Ok(self.clone())
        }
    }
}

impl Args {
    pub fn p2p_udp_bind_addr(&self) -> String {
        format!("{}:{}", self.p2p_bind_addr, self.p2p_udp_port)
    }

    pub fn security_config(&self) -> SecurityConfig {
        SecurityConfig::from_args(self)
    }
}

fn parse_client_id(s: &str) -> Result<[u8; 16], String> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(|e| format!("Invalid hex string: {}", e))?;
    Ok(bytes
        .try_into()
        .map_err(|_| format!("Invalid client ID length"))?)
}

#[derive(ValueEnum, Debug, Clone, serde::Serialize)]
pub enum WorkerType {
    #[clap(name = "tcp")]
    TCP,
    #[clap(name = "ws")]
    WS,
}

#[derive(ValueEnum, Debug, Clone, PartialEq, serde::Serialize)]
pub enum EngineType {
    #[clap(name = "vllm")]
    VLLM,
    #[clap(name = "ollama")]
    OLLAMA,
    #[clap(name = "llama")]
    LLAMA,
}

impl EngineType {
    pub fn to_common(&self) -> common::EngineType {
        match self {
            EngineType::VLLM => common::EngineType::Vllm,
            EngineType::OLLAMA => common::EngineType::Ollama,
            EngineType::LLAMA => common::EngineType::Llama,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_control_tls_flags() {
        let args = Args::try_parse_from([
            "gpuf-c",
            "--standalone-llama",
            "--control-tls",
            "--control-tls-server-name",
            "gpuf.example.internal",
        ])
        .unwrap();
        assert!(args.control_tls);
        assert_eq!(
            args.control_tls_server_name.as_deref(),
            Some("gpuf.example.internal")
        );
    }
}
