use anyhow::{anyhow, Result};
use bincode::{self as bincode, config as bincode_config, Decode, Encode};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::warn;
pub mod config;
use bytes::BytesMut;
use config::GpuModelConfig;

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
pub struct Model {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

// Device information from client to server
#[derive( Encode, Decode, Debug, Clone)]
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
#[derive( Encode, Decode, Debug, Clone)]
pub struct DevicesInfo {
    //pod info
    pub num: u16,
    pub pod_id:u16,
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
    // pub auto_models: bool,
    pub model_name: Option<String>,
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
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone,Default)]
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
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum CommandV2 {
        /// P2P connection request - gpuf-c request gpuf-s to establish P2P connection with another client
    P2PConnectionRequest {
        source_client_id: [u8; 16],
        target_client_id: [u8; 16],
        connection_id: [u8; 16],
    },
    
    /// P2P connection info - gpuf-s send peer info to both ends
    P2PConnectionInfo {
        peer_id: [u8; 16],
        peer_addrs: Vec<String>,  // multiple candidate addresses (public IP, private IP)
        stun_result: Option<String>,  // STUN discovered public address
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
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum P2PConnectionType {
    Direct,      // direct P2P connection
    Relay,       // relay through gpuf-s
    TURN,        // through TURN server
}

#[derive( Encode, Decode, Debug, Clone, PartialEq)]
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
    None = 5,
}

impl EngineType {
    pub fn to_i16(&self) -> i16 {
        match self {
            EngineType::Ollama => 1,
            EngineType::Vllm => 2,
            EngineType::TensorRT => 3,
            EngineType::ONNX => 4,
            EngineType::None => 5,
        }
    }
}

use std::fmt;
impl fmt::Display for EngineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineType::Ollama => write!(f, "Ollama"),
            EngineType::Vllm => write!(f, "vLLM"),
            EngineType::TensorRT => write!(f, "TensorRT"),
            EngineType::ONNX => write!(f, "ONNX"),
            EngineType::None => write!(f, "None"),
        }
    }
}

pub fn process_id(id: &[u8; 32]) -> &str {
    let len = id.iter().position(|&b| b == 0).unwrap_or(32);
    std::str::from_utf8(&id[..len]).unwrap_or_default()
}

// Max message size 10MB
pub const MAX_MESSAGE_SIZE: usize = 1024;

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

    let (command, _) = bincode::decode_from_slice(&buf, config)
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
];

#[inline]
pub fn os_type_str(os_type_src: &OsType) -> Option<&'static str> {
    OS_TPYE_MAP
        .iter()
        .find(|(_, os_type)| *os_type == *os_type_src)
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
        devices_info: vec![
            DevicesInfo {
                num: 0,
                pod_id: 0,
                total_tflops: 0,
                memtotal_gb: 0,
                port: 0,
                ip: 0,
                os_type: OsType::MACOS,
                engine_type: EngineType::Ollama,
                memsize_gb: 0,
                powerlimit_w: 0,
                vendor_id: 0,
                device_id: 0,
                usage: 60,
                mem_usage: 50,
                power_usage: 250,
                temp: 123,
            }
        ],
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
            println!("Command deserialized successfully cmd_v1 {:?}, deser_v1 {:?}",cmd_v1, deser_v1);

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
                    }
                ) => {
                    assert_eq!(original_id, deserialized_id, "client_id mismatch");
                    assert_eq!(original_sys.cpu_usage, deserialized_sys.cpu_usage, "cpu_usage mismatch");
                    assert_eq!(original_sys.memory_usage, deserialized_sys.memory_usage, "memory_usage mismatch");
                    assert_eq!(original_sys.disk_usage, deserialized_sys.disk_usage, "disk_usage mismatch");
                    assert_eq!(original_devices.len(), deserialized_devices.len(), "device_info length mismatch");

                    for (orig, deser) in original_devices.iter().zip(deserialized_devices) {
                        assert_eq!(orig.vendor_id, deser.vendor_id, "device vendor_id mismatch");
                        assert_eq!(orig.device_id, deser.device_id, "device device_id mismatch");
                        assert_eq!(orig.usage, deser.usage, "device usage mismatch");
                        assert_eq!(orig.mem_usage, deser.mem_usage, "device mem_usage mismatch");
                        assert_eq!(orig.power_usage, deser.power_usage, "device power_usage mismatch");
                    }
                }
                _ => panic!("Unexpected command variant"),
            }
        }
        _ => panic!("Command version mismatch"),
    }
}
