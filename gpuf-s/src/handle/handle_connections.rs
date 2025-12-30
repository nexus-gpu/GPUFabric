use super::*;

use crate::db::{
    client,
    models::{self, HotModelClass},
};
use crate::util::protoc::{ClientId, HeartbeatMessage};
use bytes::BytesMut;
use std::collections::HashMap;

use anyhow::{anyhow, Result};
use common::{os_type_str, CommandV2, Model, OsType, PodModel};
use redis::Client as RedisClient;
use sqlx::{Pool, Postgres};
use std::sync::Arc;

use tokio::net::{tcp::OwnedWriteHalf, TcpListener};

use bincode::config;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::Duration;
use tracing::{debug, error, warn};

use base64::Engine;
use hmac::{Hmac, Mac};
use sha1::Sha1;

#[cfg(unix)]
use socket2::{Socket, TcpKeepalive};
#[cfg(unix)]
use std::mem;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use tokio::net::TcpStream;

impl ServerState {
    pub async fn handle_client_connections(self: Arc<Self>, listener: TcpListener) -> Result<()> {
        loop {
            let (stream, addr) = listener.accept().await?;
            info!("New control connection from: {}", addr);
            let active_clients_clone = self.active_clients.clone();
            let db_pool_clone = self.db_pool.clone();
            let redis_client_clone = self.redis_client.clone();
            let client_models = self.client_model.clone();
            let hot_models = self.hot_models.clone();
            let producer: Arc<FutureProducer> = self.producer.clone();
            let server_state_clone = self.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_single_client(
                    stream,
                    active_clients_clone,
                    client_models,
                    hot_models,
                    db_pool_clone,
                    producer,
                    redis_client_clone,
                    server_state_clone,
                )
                .await
                {
                    error!("Error handling client {}: {}", addr, e);
                }
            });
        }
    }
}

#[cfg(unix)]
fn set_keepalive(stream: &TcpStream) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let fd = stream.as_raw_fd();
    let socket = unsafe { Socket::from_raw_fd(fd) };

    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10))
        .with_retries(3);

    let result = socket.set_tcp_keepalive(&keepalive);
    // Prevent socket from being automatically closed
    mem::forget(socket);

    result
}

#[cfg(not(unix))]
fn set_keepalive(_stream: &TcpStream) -> std::io::Result<()> {
    // Windows TCP keepalive is handled differently
    // For now, just return Ok
    Ok(())
}

async fn handle_single_client(
    stream: TcpStream,
    active_clients: ActiveClients,
    _client_models: Arc<ClientModelClass>,
    hot_models: Arc<HotModelClass>,
    db_pool: Arc<Pool<Postgres>>,
    producer: Arc<FutureProducer>,
    redis_client: Arc<RedisClient>,
    server_state: Arc<crate::handle::ServerState>,
) -> Result<()> {
    if let Err(_e) = set_keepalive(&stream) {
        error!("handle_single_client set_keepalive err");
        return Ok(());
    }
    let (mut reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let addr = reader.peer_addr().expect("Failed to get peer address");

    let mut authed = false;
    let mut session_client_id = ClientId([0; 16]);
    let mut buf = BytesMut::with_capacity(1024 * 1024);

    loop {
        match read_command(&mut reader, &mut buf).await {
            Ok(Command::V1(CommandV1::Login {
                version,
                auto_models: _,
                client_id: id,
                os_type,
                system_info,
                device_memtotal_gb,
                device_total_tflops,
                devices_info,
            })) => {
                info!("Registration attempt for client_id: {:?}", id);
                debug!(
                    "Registration attempt for devices_info: {:?} device_total_tflops {}",
                    devices_info, device_total_tflops
                );

                let validate_result = match handle_login(
                    version,
                    &active_clients,
                    &redis_client,
                    &db_pool,
                    &hot_models,
                    &ClientId(id),
                    os_type,
                    devices_info,
                    SystemInfo {
                        cpu_usage: system_info.cpu_usage,
                        memory_usage: system_info.memory_usage,
                        disk_usage: system_info.disk_usage,
                        device_memsize: device_memtotal_gb,
                        total_tflops: device_total_tflops,
                        memsize_gb: device_memtotal_gb,
                        last_heartbeat: Utc::now().into(),
                    },
                    &writer,
                    &mut authed,
                )
                .await
                {
                    Ok(validate_result) => validate_result,
                    Err(e) => {
                        error!("Failed to handle login: {}", e);
                        CommandV1::LoginResult {
                            success: false,
                            pods_model: Vec::new(),
                            error: Some(e.to_string()),
                        }
                    }
                };
                session_client_id = ClientId(id);

                write_command(&mut *writer.lock().await, &Command::V1(validate_result)).await?;
            }
            // Device system status from client to server 120s
            Ok(Command::V1(CommandV1::Heartbeat {
                client_id: id,
                system_info,
                device_memtotal_gb,
                device_total_tflops,
                device_count,
                devices_info,
            })) => {
                info!("Heartbeat received from client {}", hex::encode(id));
                handle_heartbeat(
                    &producer,
                    &ClientId(id),
                    system_info,
                    devices_info,
                    device_memtotal_gb,
                    device_count as u32,
                    device_total_tflops,
                )
                .await;
            }
            // Device model status from client to server 300s
            Ok(Command::V1(CommandV1::ModelStatus {
                client_id: id,
                models,
                auto_models_device,
            })) => {
                info!(
                    "Model status received from client {} pod num {}",
                    hex::encode(id),
                    auto_models_device.len()
                );

                let pods_model = match handle_models_status(
                    &hot_models,
                    &active_clients,
                    &ClientId(id),
                    auto_models_device,
                    models,
                )
                .await
                {
                    Ok(pods_model) => CommandV1::PullModelResult {
                        error: None,
                        pods_model,
                    },
                    Err(e) => {
                        error!("Failed to handle models status: {}", e);
                        CommandV1::PullModelResult {
                            error: Some(e.to_string()),
                            pods_model: Vec::new(),
                        }
                    }
                };
                write_command(&mut *writer.lock().await, &Command::V1(pods_model)).await?;
            }
            Err(e) => {
                info!("Client addr {} disconnected. {}", addr, e);
                active_clients.lock().await.remove(&session_client_id);
                client::upsert_client_status(&db_pool, &session_client_id, "offline").await?;
                return Ok(());
            }
            Ok(Command::V1(CommandV1::InferenceResult {
                task_id,
                success,
                result,
                error,
                execution_time_ms,
                prompt_tokens,
                completion_tokens,
            })) => {
                info!(
                    "Received inference result for task {} from device {}",
                    task_id,
                    hex::encode(&session_client_id.0)
                );
                // Route result to inference scheduler to complete HTTP response
                server_state
                    .inference_scheduler
                    .handle_inference_result(
                        task_id,
                        success,
                        result,
                        error,
                        execution_time_ms,
                        prompt_tokens,
                        completion_tokens,
                    )
                    .await;
            }
            Ok(Command::V1(CommandV1::InferenceResultChunk {
                task_id,
                seq,
                delta,
                done,
                error,
                prompt_tokens,
                completion_tokens,
            })) => {
                server_state
                    .inference_scheduler
                    .handle_inference_result_chunk(
                        task_id,
                        seq,
                        delta,
                        done,
                        error,
                        prompt_tokens,
                        completion_tokens,
                    )
                    .await;
            }

            Ok(Command::V2(CommandV2::P2PConnectionRequest {
                source_client_id,
                target_client_id,
                connection_id,
            })) => {
                if !authed {
                    return Err(anyhow!("P2PConnectionRequest before login"));
                }

                if session_client_id.0 != source_client_id {
                    return Err(anyhow!(
                        "P2PConnectionRequest source_client_id mismatch with session"
                    ));
                }

                let source_id = ClientId(source_client_id);
                let target_id = ClientId(target_client_id);

                let (source_writer, target_writer) = {
                    let clients = active_clients.lock().await;
                    let source = clients
                        .get(&source_id)
                        .map(|c| c.writer.clone())
                        .ok_or_else(|| anyhow!("Source client not online"))?;
                    let target = clients
                        .get(&target_id)
                        .map(|c| c.writer.clone())
                        .ok_or_else(|| anyhow!("Target client not online"))?;
                    (source, target)
                };

                let turn_host =
                    std::env::var("TURN_HOST").map_err(|_| anyhow!("TURN_HOST env is required"))?;
                let turn_port: u16 = std::env::var("TURN_TURNS_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5349);
                let turn_udp_port: u16 = std::env::var("TURN_TURN_UDP_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3478);
                let stun_port: u16 = std::env::var("TURN_STUN_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3478);
                let ttl_seconds: u64 = std::env::var("TURN_TTL_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(300);
                let secret = std::env::var("TURN_REST_SECRET")
                    .map_err(|_| anyhow!("TURN_REST_SECRET env is required"))?;

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| anyhow!("System time error: {e}"))?
                    .as_secs();
                let expires_at = now.saturating_add(ttl_seconds);
                let username = format!("{}:{}", expires_at, hex::encode(source_client_id));
                let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes())
                    .map_err(|e| anyhow!("Invalid TURN_REST_SECRET: {e}"))?;
                mac.update(username.as_bytes());
                let password =
                    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

                let stun_urls = vec![format!("stun:{}:{}", turn_host, stun_port)];
                let turn_urls = vec![format!(
                    "turn:{}:{}?transport=udp",
                    turn_host, turn_udp_port
                )];

                let to_source = Command::V2(CommandV2::P2PConnectionConfig {
                    peer_id: target_client_id,
                    connection_id,
                    stun_urls: stun_urls.clone(),
                    turn_urls: turn_urls.clone(),
                    turn_username: username.clone(),
                    turn_password: password.clone(),
                    expires_at,
                    force_tls: false,
                });

                let to_target = Command::V2(CommandV2::P2PConnectionConfig {
                    peer_id: source_client_id,
                    connection_id,
                    stun_urls,
                    turn_urls,
                    turn_username: username,
                    turn_password: password,
                    expires_at,
                    force_tls: false,
                });

                write_command(&mut *source_writer.lock().await, &to_source).await?;
                write_command(&mut *target_writer.lock().await, &to_target).await?;

                // Notify target about the request (optional but useful)
                let forward = Command::V2(CommandV2::P2PConnectionRequest {
                    source_client_id,
                    target_client_id,
                    connection_id,
                });
                write_command(&mut *target_writer.lock().await, &forward).await?;
            }

            Ok(Command::V2(CommandV2::P2PCandidates {
                source_client_id,
                target_client_id,
                connection_id,
                candidates,
            })) => {
                if !authed {
                    return Err(anyhow!("P2PCandidates before login"));
                }

                let src = ClientId(source_client_id);
                let dst = ClientId(target_client_id);

                // Require that the sender matches the current session.
                if session_client_id != src {
                    return Err(anyhow!("P2PCandidates source mismatch with session"));
                }

                // Minimal validation to avoid abusive payloads.
                if candidates.len() > 64 {
                    return Err(anyhow!("Too many candidates"));
                }
                for c in &candidates {
                    if c.addr.len() > 128 {
                        return Err(anyhow!("Candidate addr too long"));
                    }
                }

                let target_writer = {
                    let clients = active_clients.lock().await;
                    clients
                        .get(&dst)
                        .map(|c| c.writer.clone())
                        .ok_or_else(|| anyhow!("Target client not online"))?
                };

                let forward = Command::V2(CommandV2::P2PCandidates {
                    source_client_id,
                    target_client_id,
                    connection_id,
                    candidates,
                });
                write_command(&mut *target_writer.lock().await, &forward).await?;
            }
            _ => {
                warn!("Received unexpected command from client addr {}", addr);
            }
        }
    }
    #[allow(unreachable_code)]
    Ok(()) // This is theoretically unreachable but required by compiler
}

async fn handle_login(
    version: u32,
    active_clients: &Arc<Mutex<HashMap<ClientId, ClientInfo>>>,
    redis_client: &Arc<RedisClient>,
    db_pool: &Pool<Postgres>,
    hot_models: &Arc<HotModelClass>,
    client_id: &ClientId,
    os_type: OsType,
    devices_info: Vec<DevicesInfo>,
    system_info: SystemInfo,
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    authed: &mut bool,
) -> Result<CommandV1> {
    info!("Registration attempt for client_id: {}", client_id);
    let mut clients = active_clients.lock().await;
    if clients.contains_key(&client_id) {
        warn!("Client ID {:?} already registered.", client_id);
        return Err(anyhow!("Client ID already registered"));
    }
    debug!("Login os_type: {:?}", &os_type_str(&os_type).unwrap());

    let is_valid = client::validate_client(
        &db_pool,
        &redis_client,
        &os_type_str(&os_type).unwrap(),
        client_id,
    )
    .await?;

    let validate_result = if is_valid {
        info!("Client {} registered successfully", client_id);
        *authed = true;
        let pods_model = models::get_models_batch(&hot_models, &devices_info).await?;

        CommandV1::LoginResult {
            success: true,
            pods_model,
            error: None,
        }
    } else {
        CommandV1::LoginResult {
            success: false,
            pods_model: Vec::new(),
            error: Some("Invalid client ID".to_string()),
        }
    };

    debug!(
        "Client {} No compatible models {:#?}",
        client_id, validate_result
    );

    clients.insert(
        *client_id,
        ClientInfo {
            writer: writer.clone(),
            authed: *authed,
            version,
            system_info: Some(SystemInfo {
                cpu_usage: system_info.cpu_usage,
                memory_usage: system_info.memory_usage,
                disk_usage: system_info.disk_usage,
                device_memsize: system_info.device_memsize,
                total_tflops: system_info.total_tflops,
                memsize_gb: system_info.memsize_gb,
                last_heartbeat: Utc::now().into(),
            }),
            connected_at: Utc::now(),
            models: None,
            devices_info,
        },
    );
    Ok(validate_result)
}

async fn handle_models_status(
    hot_models: &Arc<HotModelClass>,
    active_clients: &Arc<Mutex<HashMap<ClientId, ClientInfo>>>,
    client_id: &ClientId,
    auto_models_device: Vec<DevicesInfo>,
    models: Vec<Model>,
) -> Result<Vec<PodModel>> {
    //TODO: push msg-> api filter
    let mut clients = active_clients.lock().await;
    if let Some(client) = clients.get_mut(client_id) {
        client.models = Some(models);
    }

    let mut pods_model: Vec<PodModel> = Vec::with_capacity(auto_models_device.len());

    for device in auto_models_device {
        match hot_models
            .get_hot_model(device.memtotal_gb as u32, device.engine_type.to_i16())
            .await
        {
            Ok(model_name) => {
                pods_model.push(PodModel {
                    pod_id: device.pod_id,
                    model_name: Some(model_name),
                });
            }
            Err(e) => {
                pods_model.push(PodModel {
                    pod_id: device.pod_id,
                    model_name: None,
                });
                error!("Failed to get hot model: {}", e);
            }
        };
    }

    Ok(pods_model)
}

async fn handle_heartbeat(
    producer: &Arc<FutureProducer>,
    client_id: &ClientId,
    system_info: common::SystemInfo,
    devices_info: Vec<common::DevicesInfo>,
    device_memtotal_gb: u32,
    device_count: u32,
    total_tflops: u32,
) {
    debug!("Sending heartbeat to consumer client_id {} cpu_usage {}%  memory_usage {}% disk_usage {}% device_memtotal_gb {} GB device_count {} total_tflops {} tflops", client_id, system_info.cpu_usage, system_info.memory_usage, system_info.disk_usage, device_memtotal_gb, device_count, total_tflops);

    let heartbeat_message = HeartbeatMessage {
        client_id: client_id.clone(),
        device_memtotal_gb,
        device_count,
        total_tflops,
        system_info,
        devices_info,
    };

    let cfg = config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();

    let heartbeat_message_bytes = bincode::encode_to_vec(&heartbeat_message, cfg).unwrap();
    if let Err(e) = producer
        .send(
            FutureRecord::to("client-heartbeats")
                .payload(&heartbeat_message_bytes)
                .key(&client_id.to_string()),
            Duration::from_secs(0),
        )
        .await
    {
        error!("Failed to send heartbeat to Kafka: {:?}", e);
    };
}

#[cfg(test)]
mod tests {}
