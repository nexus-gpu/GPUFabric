use anyhow::{anyhow, Result};
use bincode::{self as bincode, config as bincode_config, Decode, Encode};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::warn;
pub mod config;
use bytes::BytesMut;
use config::GpuModelConfig;
use std::fmt;
use zeroize::Zeroize;

pub const COMMAND_V1_BASE_VERSION: u32 = 1;
pub const COMMAND_V1_EMBEDDING_TASKS_VERSION: u32 = 2;
pub const CURRENT_COMMAND_V1_VERSION: u32 = COMMAND_V1_EMBEDDING_TASKS_VERSION;

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct Model {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

// Device information from client to server
#[derive(Encode, Decode, Debug, Clone)]
pub struct DeviceInfo {
    pub index: u8,
    pub usage: u8,
    pub mem_usage: u8,
    pub power_usage: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub temp: u32,
}

// Device information from client to server (max num: 8)
#[derive(Encode, Decode, Debug, Clone)]
pub struct DevicesInfo {
    //pod info
    pub num: u16,
    pub pod_id: u16,
    pub total_tflops: u16,
    pub memtotal_gb: u16,
    pub port: u16,
    pub ip: u32,
    pub os_type: OsType,
    pub engine_type: EngineType,

    //device info
    pub usage: u64,
    pub mem_usage: u64,
    pub power_usage: u64,
    pub temp: u64,
    pub vendor_id: u128,
    pub device_id: u128,
    pub memsize_gb: u128,
    pub powerlimit_w: u128,
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct PodModel {
    pub pod_id: u16,
    pub model_name: Option<String>,
    pub download_url: Option<String>,
    pub checksum: Option<String>,
    pub expected_size: Option<u64>,
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct ChatMessageV2 {
    pub role: String,
    pub content: ChatMessageContent,
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

impl ChatMessageContent {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Parts(_) => None,
        }
    }

    pub fn is_multimodal(&self) -> bool {
        matches!(self, Self::Parts(parts) if parts.iter().any(ChatContentPart::is_image_like))
    }
}

impl From<String> for ChatMessageContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct ChatContentPart {
    pub r#type: String,
    pub text: Option<String>,
    pub image_url: Option<ImageUrlValue>,
    pub image: Option<String>,
}

impl ChatContentPart {
    pub fn is_image_like(&self) -> bool {
        matches!(self.r#type.as_str(), "image_url" | "image")
    }
}

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
#[serde(untagged)]
pub enum ImageUrlValue {
    Url(String),
    Object { url: String, detail: Option<String> },
}

impl ImageUrlValue {
    pub fn url(&self) -> &str {
        match self {
            Self::Url(url) => url,
            Self::Object { url, .. } => url,
        }
    }
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPhase {
    Unknown,
    Analysis,
    Final,
}

impl Default for OutputPhase {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Default for DevicesInfo {
    fn default() -> Self {
        Self {
            num: 0,
            pod_id: 0,
            total_tflops: 0,
            memtotal_gb: 0,
            port: 0,
            ip: 0,
            os_type: OsType::NONE,
            engine_type: EngineType::None,
            usage: 0,
            mem_usage: 0,
            power_usage: 0,
            temp: 0,
            vendor_id: 0,
            device_id: 0,
            memsize_gb: 0,
            powerlimit_w: 0,
        }
    }
}

#[inline]
pub fn get_u16_from_u128(value: u128, index: usize) -> u16 {
    assert!(index < 8);
    ((value >> (index * 16)) & 0xFFFF) as u16
}

#[inline]
pub fn set_u16_to_u128(value: &mut u128, index: usize, val: u16) {
    assert!(index < 8);
    let shift = index * 16;
    let mask = !(0xFFFF << shift);
    *value &= mask;
    *value |= (val as u128) << shift;
}

#[inline]
pub fn get_u8_from_u64(value: u64, index: usize) -> u8 {
    assert!(index < 8);
    ((value >> (index * 8)) & 0xFF) as u8
}

#[inline]
pub fn set_u8_to_u64(value: &mut u64, index: usize, val: u8) {
    assert!(index < 8);
    let shift = index * 8;
    let mask = !(0xFF << shift);
    *value &= mask;
    *value |= (val as u64) << shift;
}

/// System information from client to server
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone, Default)]
pub struct SystemInfo {
    pub cpu_usage: u8,
    pub memory_usage: u8,
    pub disk_usage: u8,
    pub network_rx: u64,
    pub network_tx: u64,
}

/// Commands exchanged between client and server.
#[derive(Encode, Decode, Debug, Clone)]
pub enum Command {
    V1(CommandV1),
    V2(CommandV2),
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum CommandV1 {
    /// Request a new proxy connection. Sent from gpuf-s to a chosen gpuf-c.
    RequestNewProxyConn {
        proxy_conn_id: [u8; 16],
    },
    /// Notify the proxy listener that a new client is ready. Sent from gpuf-c to gpuf-s.
    NewProxyConn {
        proxy_conn_id: [u8; 16],
    },

    // Login with client id and system info and device info
    Login {
        client_id: [u8; 16],
        version: u32,
        os_type: OsType,
        auto_models: bool,
        system_info: SystemInfo,
        device_memtotal_gb: u32,
        device_total_tflops: u32,
        devices_info: Vec<DevicesInfo>,
    },
    LoginResult {
        success: bool,
        pods_model: Vec<PodModel>,
        error: Option<String>,
    },

    // System status from client to server 120s
    Heartbeat {
        client_id: [u8; 16],
        system_info: SystemInfo,
        device_count: u16,
        device_memtotal_gb: u32,
        device_total_tflops: u32,
        devices_info: Vec<DevicesInfo>,
    },

    // Push model to server
    PullModelResult {
        pods_model: Vec<PodModel>,
        error: Option<String>,
    },

    // Model info from client to server 300s
    ModelStatus {
        client_id: [u8; 16],
        models: Vec<Model>,
        auto_models_device: Vec<DevicesInfo>,
    },

    // Inference task from server to client
    InferenceTask {
        task_id: String,
        session_id: Option<String>,
        cache_policy: Option<String>,
        prompt: String,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    },

    // Chat inference task from server to client
    ChatInferenceTask {
        task_id: String,
        session_id: Option<String>,
        cache_policy: Option<String>,
        model: String,
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    },

    CancelInference {
        task_id: String,
    },

    // Inference result from client to server
    InferenceResult {
        task_id: String,
        success: bool,
        result: Option<String>,
        error: Option<String>,
        execution_time_ms: u64,
        prompt_tokens: u32,
        completion_tokens: u32,
    },

    InferenceResultChunk {
        task_id: String,
        seq: u32,
        delta: String,
        phase: OutputPhase,
        done: bool,
        error: Option<String>,
        prompt_tokens: u32,
        completion_tokens: u32,
        analysis_tokens: u32,
        final_tokens: u32,
    },

    // Model download progress from client to server
    ModelDownloadProgress {
        client_id: [u8; 16],
        model_name: String,
        downloaded_bytes: u64,
        total_bytes: u64,
        percentage: f32,
        speed_bps: u64,
        status: DownloadStatus,
        error: Option<String>,
    },

    // Embedding task from server to client. Keep new variants at the end so
    // older CommandV1 discriminants stay compatible.
    EmbeddingTask {
        task_id: String,
        model: String,
        input: Vec<String>,
        normalize: bool,
    },

    EmbeddingResult {
        task_id: String,
        success: bool,
        embeddings: Vec<Vec<f32>>,
        error: Option<String>,
        prompt_tokens: u32,
    },
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum CommandV2 {
    /// P2P connection request - gpuf-c request gpuf-s to establish P2P connection with another client
    P2PConnectionRequest {
        source_client_id: [u8; 16],
        target_client_id: [u8; 16],
        connection_id: [u8; 16],
    },

    P2PConnectionConfig {
        peer_id: [u8; 16],
        connection_id: [u8; 16],
        stun_urls: Vec<String>,
        turn_urls: Vec<String>,
        turn_username: String,
        turn_password: RedactedString,
        data_plane_secret: DataPlaneSecret,
        expires_at: u64,
        force_tls: bool,
    },

    P2PCandidates {
        source_client_id: [u8; 16],
        target_client_id: [u8; 16],
        connection_id: [u8; 16],
        candidates: Vec<P2PCandidate>,
    },

    P2PDataPlaneEnvelope {
        connection_id: [u8; 16],
        seq: u64,
        timestamp: u64,
        payload: Vec<u8>,
        tag: [u8; 32],
    },

    P2PInferenceRequest {
        connection_id: [u8; 16],
        task_id: String,
        model: Option<String>,
        prompt: String,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    },

    P2PInferenceChunk {
        connection_id: [u8; 16],
        task_id: String,
        seq: u32,
        delta: String,
        phase: OutputPhase,
        done: bool,
        error: Option<String>,
        analysis_tokens: u32,
        final_tokens: u32,
    },

    P2PInferenceDone {
        connection_id: [u8; 16],
        task_id: String,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
        analysis_tokens: u32,
        final_tokens: u32,
    },

    P2PCancelInference {
        connection_id: [u8; 16],
        task_id: String,
    },

    /// P2P connection info - gpuf-s send peer info to both ends
    P2PConnectionInfo {
        peer_id: [u8; 16],
        peer_addrs: Vec<String>, // multiple candidate addresses (public IP, private IP)
        stun_result: Option<String>, // STUN discovered public address
        connection_id: [u8; 16],
    },

    /// P2P connection established - gpuf-c and gpuf-s establish P2P connection
    P2PConnectionEstablished {
        peer_id: [u8; 16],
        connection_id: [u8; 16],
        connection_type: P2PConnectionType,
    },

    /// P2P connection failed, fallback to relay mode
    P2PConnectionFailed {
        peer_id: [u8; 16],
        connection_id: [u8; 16],
        error: String,
    },

    /// Chat inference task with OpenAI-compatible multimodal content parts.
    ChatInferenceTask {
        task_id: String,
        session_id: Option<String>,
        cache_policy: Option<String>,
        model: String,
        messages: Vec<ChatMessageV2>,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    },

    /// P2P file transfer start metadata for OCR/chat attachments.
    P2PFileStart {
        connection_id: [u8; 16],
        transfer_id: [u8; 16],
        task_id: Option<String>,
        purpose: P2PFilePurpose,
        file_name: Option<String>,
        mime_type: String,
        byte_size: u64,
        sha256: [u8; 32],
    },

    /// P2P file payload chunk. Integrity is covered by the signed data-plane envelope.
    P2PFileChunk {
        connection_id: [u8; 16],
        transfer_id: [u8; 16],
        seq: u32,
        offset: u64,
        data: Vec<u8>,
    },

    /// P2P file transfer completion marker.
    P2PFileDone {
        connection_id: [u8; 16],
        transfer_id: [u8; 16],
        byte_size: u64,
        sha256: [u8; 32],
    },

    /// P2P file transfer acknowledgement for flow control and resume.
    P2PFileAck {
        connection_id: [u8; 16],
        transfer_id: [u8; 16],
        received_bytes: u64,
        next_offset: u64,
        done: bool,
        error: Option<String>,
    },

    /// P2P file transfer cancellation.
    P2PFileCancel {
        connection_id: [u8; 16],
        transfer_id: [u8; 16],
        reason: String,
    },

    /// Consumer-only P2P session login. This is used by lightweight SDK/proxy
    /// clients that need to initiate P2P data-plane sessions but must not be
    /// registered as compute devices.
    P2PConsumerLogin {
        consumer_id: [u8; 16],
        api_token: RedactedString,
    },

    P2PConsumerLoginResult {
        success: bool,
        error: Option<String>,
    },

    /// Usage reported by an authenticated consumer-only P2P proxy after a
    /// real data-plane request completes. gpuf-s must bind this to the
    /// authenticated consumer session; token_hash/client ownership is never
    /// trusted from this payload.
    P2PUsageReport {
        consumer_id: [u8; 16],
        target_client_id: [u8; 16],
        connection_id: [u8; 16],
        task_id: String,
        request_id: Option<String>,
        model: String,
        endpoint: String,
        transport: P2PUsageTransport,
        stream: bool,
        multimodal: bool,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
        analysis_tokens: u32,
        final_tokens: u32,
        bytes_up: u64,
        bytes_down: u64,
        chunk_count: u32,
        retry_count: u32,
        connect_ms: u64,
        ttft_ms: Option<u64>,
        total_ms: u64,
        success: bool,
        error: Option<String>,
        output_sha256: Option<[u8; 32]>,
    },

    /// Usage receipt emitted by the target compute client. gpuf-s records
    /// consumer P2P usage only when this receipt matches the consumer report.
    P2PUsageReceipt {
        source_client_id: [u8; 16],
        target_client_id: [u8; 16],
        connection_id: [u8; 16],
        task_id: String,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
        analysis_tokens: u32,
        final_tokens: u32,
        success: bool,
        error: Option<String>,
        output_sha256: Option<[u8; 32]>,
    },
}

#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq)]
pub enum P2PFilePurpose {
    OcrImage,
    ChatImage,
    GenericAttachment,
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum P2PUsageTransport {
    DirectUdp,
    RelayUdp,
    FallbackHttp,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq)]
pub struct RedactedString(pub String);

impl fmt::Debug for RedactedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl RedactedString {
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl Drop for RedactedString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl From<String> for RedactedString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Eq)]
pub struct DataPlaneSecret(pub [u8; 32]);

impl fmt::Debug for DataPlaneSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted-data-plane-secret>")
    }
}

impl DataPlaneSecret {
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn into_inner(mut self) -> [u8; 32] {
        let out = self.0;
        self.0.zeroize();
        out
    }
}

impl Drop for DataPlaneSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub struct P2PCandidate {
    pub candidate_type: P2PCandidateType,
    pub transport: P2PTransport,
    pub addr: String,
    pub priority: u32,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum P2PTransport {
    Tcp,
    Udp,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum P2PCandidateType {
    Host,
    Srflx,
    Relay,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum P2PConnectionType {
    Direct, // direct P2P connection
    Relay,  // relay through gpuf-s
    TURN,   // through TURN server
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum OsType {
    MACOS,
    WINDOWS,
    LINUX,
    ANDROID,
    IOS,
    NONE,
}

#[repr(i16)]
#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq)]
pub enum EngineType {
    Ollama = 1,
    Vllm = 2,
    TensorRT = 3,
    ONNX = 4,
    Llama = 6,
    None = 5,
}

impl EngineType {
    pub fn to_i16(&self) -> i16 {
        match self {
            EngineType::Ollama => 1,
            EngineType::Vllm => 2,
            EngineType::TensorRT => 3,
            EngineType::ONNX => 4,
            EngineType::Llama => 6,
            EngineType::None => 5,
        }
    }
}

impl fmt::Display for EngineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineType::Ollama => write!(f, "Ollama"),
            EngineType::Vllm => write!(f, "vLLM"),
            EngineType::TensorRT => write!(f, "TensorRT"),
            EngineType::ONNX => write!(f, "ONNX"),
            EngineType::Llama => write!(f, "Llama"),
            EngineType::None => write!(f, "None"),
        }
    }
}

pub fn process_id(id: &[u8; 32]) -> &str {
    let len = id.iter().position(|&b| b == 0).unwrap_or(32);
    std::str::from_utf8(&id[..len]).unwrap_or_default()
}

// Max message size 10MB
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Reads a command from an async reader.
/// The format is a 4-byte length prefix (u32) followed by the bin-encoded command.
pub async fn read_command<R: AsyncRead + Unpin>(
    reader: &mut R,
    buf: &mut BytesMut,
) -> Result<Command> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        warn!(
            "read_command: Message too large: {} bytes (max: {} bytes)",
            len, MAX_MESSAGE_SIZE
        );
        return Err(anyhow!("Message too large"));
    }

    let config = bincode_config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();

    buf.clear();
    buf.resize(len, 0);
    reader.read_exact(buf).await?;

    let (command, _) = bincode::decode_from_slice(buf.as_ref(), config)
        .map_err(|e| anyhow!("Failed to deserialize command: {}", e))?;
    Ok(command)
}

/// Writes a command to an async writer.
/// The format is a 4-byte length prefix (u32) followed by the JSON-encoded command.
pub async fn write_command<W: AsyncWrite + Unpin>(writer: &mut W, command: &Command) -> Result<()> {
    let config = bincode_config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();
    let buf = bincode::encode_to_vec(command, config)?;
    let len = buf.len() as u32;
    if len as usize > MAX_MESSAGE_SIZE {
        warn!(
            "write_command: Message too large: {} bytes (max: {} bytes)",
            len, MAX_MESSAGE_SIZE
        );
        return Err(anyhow!("Message too large"));
    }

    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

/// Synchronous version: Reads a command from a blocking reader.
/// The format is a 4-byte length prefix (u32) followed by the bincode-encoded command.
pub fn read_command_sync<R: std::io::Read>(reader: &mut R) -> Result<Command> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_MESSAGE_SIZE {
        warn!(
            "read_command_sync: Message too large: {} bytes (max: {} bytes)",
            len, MAX_MESSAGE_SIZE
        );
        return Err(anyhow!("Message too large"));
    }

    let config = bincode_config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;

    let (command, _) = bincode::decode_from_slice(&buf, config)
        .map_err(|e| anyhow!("Failed to deserialize command: {}", e))?;
    Ok(command)
}

/// Synchronous version: Writes a command to a blocking writer.
/// The format is a 4-byte length prefix (u32) followed by the bincode-encoded command.
pub fn write_command_sync<W: std::io::Write>(writer: &mut W, command: &Command) -> Result<()> {
    let config = bincode_config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();
    let buf = bincode::encode_to_vec(command, config)?;
    let len = buf.len() as u32;

    if len as usize > MAX_MESSAGE_SIZE {
        warn!(
            "write_command_sync: Message too large: {} bytes (max: {} bytes)",
            len, MAX_MESSAGE_SIZE
        );
        return Err(anyhow!("Message too large"));
    }

    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&buf)?;
    writer.flush()?;
    Ok(())
}

/// Joins two streams, copying data in both directions.
pub async fn join_streams<A, B>(a: A, b: B) -> std::io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (mut a_reader, mut a_writer) = tokio::io::split(a);
    let (mut b_reader, mut b_writer) = tokio::io::split(b);
    let a_to_b = async {
        let result = tokio::io::copy(&mut a_reader, &mut b_writer).await;
        let _ = b_writer.shutdown().await;
        result
    };

    let b_to_a = async {
        let result = tokio::io::copy(&mut b_reader, &mut a_writer).await;
        let _ = a_writer.shutdown().await;
        result
    };
    tokio::select! {
        res = a_to_b => res?,
        res = b_to_a => res?,
    };
    Ok(())
}

//TODO: vendor to id apple and apple
const VENDOR_TO_ID: &[(&str, u16)] = &[
    ("Apple", 0x106b),
    ("Apple", 0x6810),
    ("Intel", 0x8086),
    ("AMD", 0x1022),
    ("AMD", 0x1002),
    ("NVIDIA", 0x10de),
];

pub fn vendor_to_id(vendor: &str) -> Option<u16> {
    VENDOR_TO_ID
        .iter()
        .find(|(s, _)| *s == vendor)
        .map(|(_, id)| *id)
}
pub fn id_to_vendor(id: u16) -> Option<&'static str> {
    VENDOR_TO_ID.iter().find(|(_, i)| *i == id).map(|(s, _)| *s)
}
//mac/linux/wins
const OS_TPYE_MAP: &[(&str, OsType)] = &[
    ("mac", OsType::MACOS),
    ("linux", OsType::LINUX),
    ("win", OsType::WINDOWS),
    ("android", OsType::ANDROID),
    ("ios", OsType::IOS),
];

#[inline]
pub fn os_type_str(os_type_src: &OsType) -> Option<&'static str> {
    OS_TPYE_MAP
        .iter()
        .find(|(_, os_type)| os_type == os_type_src)
        .map(|(s, _)| *s)
}

use lazy_static::lazy_static;
lazy_static! {
    pub static ref GPU_CONFIG: GpuModelConfig =
        GpuModelConfig::load().expect("Failed to load GPU config");
}

pub fn model_to_id(model: &str) -> Option<u16> {
    GPU_CONFIG.get_id(model)
}

pub fn id_to_model(id: u16) -> Option<String> {
    GPU_CONFIG
        .model_to_id
        .iter()
        .find_map(|(k, &v)| if v == id { Some(k.clone()) } else { None })
}

pub fn to_tflops(id: u16) -> Option<f32> {
    GPU_CONFIG.get_tflops(id)
}

#[macro_export]
macro_rules! format_bytes {
    ($bytes:expr) => {{
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;
        let bytes = $bytes as f64;

        if bytes >= TB as f64 {
            format!("{:.2} TB", bytes / TB as f64)
        } else if bytes >= GB as f64 {
            format!("{:.2} GB", bytes / GB as f64)
        } else if bytes >= MB as f64 {
            format!("{:.2} MB", bytes / MB as f64)
        } else if bytes >= KB as f64 {
            format!("{:.2} KB", bytes / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }};
}

#[macro_export]
macro_rules! format_duration {
    ($seconds:expr) => {{
        const MINUTE: u64 = 60;
        const HOUR: u64 = MINUTE * 60;
        const DAY: u64 = HOUR * 24;
        let seconds = $seconds;

        if seconds >= DAY {
            let days = seconds / DAY;
            let hours = (seconds % DAY) / HOUR;
            format!("{}d {}h", days, hours)
        } else if seconds >= HOUR {
            let hours = seconds / HOUR;
            let minutes = (seconds % HOUR) / MINUTE;
            format!("{}h {}m", hours, minutes)
        } else if seconds >= MINUTE {
            let minutes = seconds / MINUTE;
            let secs = seconds % MINUTE;
            format!("{}m {}s", minutes, secs)
        } else {
            format!("{}s", seconds)
        }
    }};
}

#[test]
fn test_model_to_id() {
    assert_eq!(model_to_id("Apple M1"), Some(0x0001));
    assert_eq!(model_to_id("Apple M1 Pro"), Some(0x0002));
    assert_eq!(model_to_id("Apple M1 Max"), Some(0x0003));
    assert_eq!(model_to_id("Apple M1 Ultra"), Some(0x0004));
    assert_eq!(model_to_id("Apple M2"), Some(0x0005));
    assert_eq!(id_to_model(0x0001), Some("Apple M1".to_string()));
    assert_eq!(id_to_model(0x0002), Some("Apple M1 Pro".to_string()));
    assert_eq!(id_to_model(0x0003), Some("Apple M1 Max".to_string()));
    assert_eq!(id_to_model(0x0004), Some("Apple M1 Ultra".to_string()));
    assert_eq!(id_to_model(0x0005), Some("Apple M2".to_string()));
}

#[test]
fn test_id_to_model() {
    assert_eq!(vendor_to_id("Apple"), Some(0x106b));
    assert_eq!(vendor_to_id("Intel"), Some(0x8086));
    assert_eq!(vendor_to_id("AMD"), Some(0x1022));
    assert_eq!(vendor_to_id("NVIDIA"), Some(0x10de));
}

#[test]
fn test_format_bytes() {
    let mut value = 0;
    set_u8_to_u64(&mut value, 0, 255);
    println!("value: {}", value);
    let value2 = 255;
    println!("value2: {}", value2);
    assert_eq!(value, value2);
}

#[tokio::test]
async fn test_command_serialization_roundtrip() {
    // Create a Vec<u8> buffer for writing
    let mut buf = Vec::with_capacity(MAX_MESSAGE_SIZE);
    // Create a BufWriter that wraps our Vec<u8>
    let mut writer = tokio::io::BufWriter::new(&mut buf);

    // Test data using CommandV1
    let cmd = Command::V1(CommandV1::Login {
        auto_models: false,
        client_id: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        os_type: OsType::MACOS,
        system_info: SystemInfo {
            cpu_usage: 50,
            memory_usage: 75,
            disk_usage: 25,
            network_rx: 0,
            network_tx: 0,
        },
        version: 1,
        device_memtotal_gb: 256,
        device_total_tflops: 0,
        devices_info: vec![DevicesInfo {
            num: 0,
            pod_id: 0,
            total_tflops: 0,
            memtotal_gb: 0,
            port: 0,
            ip: 0,
            os_type: OsType::MACOS,
            engine_type: EngineType::Llama,
            memsize_gb: 0,
            powerlimit_w: 0,
            vendor_id: 0,
            device_id: 0,
            usage: 60,
            mem_usage: 50,
            power_usage: 250,
            temp: 123,
        }],
    });

    // Serialize and write the command
    write_command(&mut writer, &cmd).await.unwrap();
    // Flush to ensure all data is written
    writer.flush().await.unwrap();
    // Get the written data
    let written_data = writer.into_inner();
    // Create a reader from the written data
    let mut reader = std::io::Cursor::new(&written_data[..]);
    let mut read_buf = BytesMut::with_capacity(MAX_MESSAGE_SIZE);
    // Read back the command
    let deserialized_cmd = read_command(&mut reader, &mut read_buf).await.unwrap();

    // Verify the round-trip
    match (&cmd, &deserialized_cmd) {
        (Command::V1(cmd_v1), Command::V1(deser_v1)) => {
            println!(
                "Command deserialized successfully cmd_v1 {:?}, deser_v1 {:?}",
                cmd_v1, deser_v1
            );

            match (cmd_v1, deser_v1) {
                (
                    CommandV1::Login {
                        auto_models: _,
                        client_id: original_id,
                        os_type: _,
                        system_info: original_sys,
                        devices_info: original_devices,
                        version: _,
                        device_memtotal_gb: _,
                        device_total_tflops: _,
                    },
                    CommandV1::Login {
                        auto_models: _,
                        client_id: deserialized_id,
                        os_type: _,
                        system_info: deserialized_sys,
                        devices_info: deserialized_devices,
                        version: _,
                        device_memtotal_gb: _,
                        device_total_tflops: _,
                    },
                ) => {
                    assert_eq!(original_id, deserialized_id, "client_id mismatch");
                    assert_eq!(
                        original_sys.cpu_usage, deserialized_sys.cpu_usage,
                        "cpu_usage mismatch"
                    );
                    assert_eq!(
                        original_sys.memory_usage, deserialized_sys.memory_usage,
                        "memory_usage mismatch"
                    );
                    assert_eq!(
                        original_sys.disk_usage, deserialized_sys.disk_usage,
                        "disk_usage mismatch"
                    );
                    assert_eq!(
                        original_devices.len(),
                        deserialized_devices.len(),
                        "device_info length mismatch"
                    );

                    for (orig, deser) in original_devices.iter().zip(deserialized_devices) {
                        assert_eq!(orig.vendor_id, deser.vendor_id, "device vendor_id mismatch");
                        assert_eq!(orig.device_id, deser.device_id, "device device_id mismatch");
                        assert_eq!(orig.usage, deser.usage, "device usage mismatch");
                        assert_eq!(orig.mem_usage, deser.mem_usage, "device mem_usage mismatch");
                        assert_eq!(
                            orig.power_usage, deser.power_usage,
                            "device power_usage mismatch"
                        );
                    }
                }
                _ => panic!("Unexpected command variant"),
            }
        }
        _ => panic!("Command version mismatch"),
    }
}

#[tokio::test]
async fn test_inference_task_preserves_session_fields() {
    let inference_cmd = Command::V1(CommandV1::InferenceTask {
        task_id: "task-1".to_string(),
        session_id: Some("session-1234567890".to_string()),
        cache_policy: Some("reset".to_string()),
        prompt: "hello".to_string(),
        max_tokens: 16,
        temperature: 0.2,
        top_k: 8,
        top_p: 0.9,
        repeat_penalty: 1.05,
        repeat_last_n: 32,
        min_keep: 1,
    });

    let chat_cmd = Command::V1(CommandV1::ChatInferenceTask {
        task_id: "task-2".to_string(),
        session_id: Some("session-2234567890".to_string()),
        cache_policy: Some("bypass".to_string()),
        model: "gpuf".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: "ping".to_string(),
        }],
        max_tokens: 32,
        temperature: 0.3,
        top_k: 16,
        top_p: 0.95,
        repeat_penalty: 1.0,
        repeat_last_n: 8,
        min_keep: 1,
    });

    let config = bincode_config::standard()
        .with_big_endian()
        .with_fixed_int_encoding();

    let encoded_inference = bincode::encode_to_vec(&inference_cmd, config).unwrap();
    let (decoded_inference, _) =
        bincode::decode_from_slice::<Command, _>(&encoded_inference, config).unwrap();

    let encoded_chat = bincode::encode_to_vec(&chat_cmd, config).unwrap();
    let (decoded_chat, _) =
        bincode::decode_from_slice::<Command, _>(&encoded_chat, config).unwrap();

    match decoded_inference {
        Command::V1(CommandV1::InferenceTask {
            session_id,
            cache_policy,
            ..
        }) => {
            assert_eq!(session_id.as_deref(), Some("session-1234567890"));
            assert_eq!(cache_policy.as_deref(), Some("reset"));
        }
        _ => panic!("unexpected inference variant"),
    }

    match decoded_chat {
        Command::V1(CommandV1::ChatInferenceTask {
            session_id,
            cache_policy,
            ..
        }) => {
            assert_eq!(session_id.as_deref(), Some("session-2234567890"));
            assert_eq!(cache_policy.as_deref(), Some("bypass"));
        }
        _ => panic!("unexpected chat variant"),
    }
}

#[tokio::test]
async fn test_chat_inference_v2_multimodal_roundtrip() {
    let chat_cmd = Command::V2(CommandV2::ChatInferenceTask {
        task_id: "task-mm-1".to_string(),
        session_id: None,
        cache_policy: None,
        model: "PaddleOCR-VL-1.6-GGUF".to_string(),
        messages: vec![ChatMessageV2 {
            role: "user".to_string(),
            content: ChatMessageContent::Parts(vec![
                ChatContentPart {
                    r#type: "text".to_string(),
                    text: Some("OCR this image".to_string()),
                    image_url: None,
                    image: None,
                },
                ChatContentPart {
                    r#type: "image_url".to_string(),
                    text: None,
                    image_url: Some(ImageUrlValue::Object {
                        url: "file:///tmp/demo.png".to_string(),
                        detail: Some("auto".to_string()),
                    }),
                    image: None,
                },
            ]),
        }],
        max_tokens: 32,
        temperature: 0.0,
        top_k: 16,
        top_p: 0.95,
        repeat_penalty: 1.0,
        repeat_last_n: 8,
        min_keep: 1,
    });

    let config = bincode_config::standard()
        .with_big_endian()
        .with_fixed_int_encoding();
    let encoded = bincode::encode_to_vec(&chat_cmd, config).unwrap();
    let (decoded, _) = bincode::decode_from_slice::<Command, _>(&encoded, config).unwrap();

    match decoded {
        Command::V2(CommandV2::ChatInferenceTask { messages, .. }) => {
            assert_eq!(messages.len(), 1);
            assert!(messages[0].content.is_multimodal());
        }
        _ => panic!("unexpected chat v2 variant"),
    }
}

#[tokio::test]
async fn test_p2p_file_transfer_roundtrip() {
    let connection_id = [1u8; 16];
    let transfer_id = [2u8; 16];
    let sha256 = [3u8; 32];
    let start = Command::V2(CommandV2::P2PFileStart {
        connection_id,
        transfer_id,
        task_id: Some("ocr-task-1".to_string()),
        purpose: P2PFilePurpose::OcrImage,
        file_name: Some("screen.png".to_string()),
        mime_type: "image/png".to_string(),
        byte_size: 6,
        sha256,
    });
    let chunk = Command::V2(CommandV2::P2PFileChunk {
        connection_id,
        transfer_id,
        seq: 0,
        offset: 0,
        data: b"screen".to_vec(),
    });
    let done = Command::V2(CommandV2::P2PFileDone {
        connection_id,
        transfer_id,
        byte_size: 6,
        sha256,
    });
    let ack = Command::V2(CommandV2::P2PFileAck {
        connection_id,
        transfer_id,
        received_bytes: 6,
        next_offset: 6,
        done: true,
        error: None,
    });

    let config = bincode_config::standard()
        .with_big_endian()
        .with_fixed_int_encoding();

    for command in [start, chunk, done, ack] {
        let encoded = bincode::encode_to_vec(&command, config).unwrap();
        let (decoded, _) = bincode::decode_from_slice::<Command, _>(&encoded, config).unwrap();
        match decoded {
            Command::V2(CommandV2::P2PFileStart {
                purpose,
                mime_type,
                byte_size,
                ..
            }) => {
                assert_eq!(purpose, P2PFilePurpose::OcrImage);
                assert_eq!(mime_type, "image/png");
                assert_eq!(byte_size, 6);
            }
            Command::V2(CommandV2::P2PFileChunk { data, offset, .. }) => {
                assert_eq!(offset, 0);
                assert_eq!(data, b"screen");
            }
            Command::V2(CommandV2::P2PFileDone { sha256, .. }) => {
                assert_eq!(sha256, [3u8; 32]);
            }
            Command::V2(CommandV2::P2PFileAck { done, error, .. }) => {
                assert!(done);
                assert!(error.is_none());
            }
            _ => panic!("unexpected p2p file variant"),
        }
    }
}

#[tokio::test]
async fn test_embedding_task_result_roundtrip() {
    let task = Command::V1(CommandV1::EmbeddingTask {
        task_id: "embed-task-1".to_string(),
        model: "bge-m3-q8_0".to_string(),
        input: vec!["hello".to_string(), "world".to_string()],
        normalize: true,
    });

    let result = Command::V1(CommandV1::EmbeddingResult {
        task_id: "embed-task-1".to_string(),
        success: true,
        embeddings: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
        error: None,
        prompt_tokens: 4,
    });

    let config = bincode_config::standard()
        .with_big_endian()
        .with_fixed_int_encoding();

    let encoded_task = bincode::encode_to_vec(&task, config).unwrap();
    let (decoded_task, _) =
        bincode::decode_from_slice::<Command, _>(&encoded_task, config).unwrap();

    let encoded_result = bincode::encode_to_vec(&result, config).unwrap();
    let (decoded_result, _) =
        bincode::decode_from_slice::<Command, _>(&encoded_result, config).unwrap();

    match decoded_task {
        Command::V1(CommandV1::EmbeddingTask {
            task_id,
            model,
            input,
            normalize,
        }) => {
            assert_eq!(task_id, "embed-task-1");
            assert_eq!(model, "bge-m3-q8_0");
            assert_eq!(input.len(), 2);
            assert!(normalize);
        }
        _ => panic!("unexpected embedding task variant"),
    }

    match decoded_result {
        Command::V1(CommandV1::EmbeddingResult {
            task_id,
            success,
            embeddings,
            error,
            prompt_tokens,
        }) => {
            assert_eq!(task_id, "embed-task-1");
            assert!(success);
            assert_eq!(embeddings.len(), 2);
            assert!(error.is_none());
            assert_eq!(prompt_tokens, 4);
        }
        _ => panic!("unexpected embedding result variant"),
    }
}
