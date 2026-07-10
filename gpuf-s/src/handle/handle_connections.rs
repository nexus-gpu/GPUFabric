use super::*;

use crate::db::{
    client,
    models::{self, HotModelClass},
    token_usage::{insert_token_usage, TokenUsageInsert},
};
use crate::util::geo;
use crate::util::protoc::{ClientId, HeartbeatMessage};
use bytes::BytesMut;
use std::collections::HashMap;

use anyhow::{anyhow, Result};
use common::{
    format_bytes, os_type_str, CommandV2, DataPlaneSecret, DownloadStatus, Model, OsType,
    P2PUsageTransport, PodModel, RedactedString,
};
use redis::AsyncCommands;
use redis::Client as RedisClient;
use sqlx::{Pool, Postgres};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;

use bincode::config;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::Duration;
use tokio_rustls::{rustls::server::ServerConfig as RustlsServerConfig, TlsAcceptor};
use tracing::{debug, error, info, warn};

use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;
use sha2::{Digest, Sha256};

#[cfg(unix)]
use socket2::{Socket, TcpKeepalive};
#[cfg(unix)]
use std::mem;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use tokio::net::TcpStream;

impl ServerState {
    pub async fn handle_client_connections(self: Arc<Self>, listener: TcpListener) -> Result<()> {
        let acceptor = if self.config.control_tls {
            install_rustls_crypto_provider_once();
            let server_config = RustlsServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(self.cert_chain.to_vec(), self.priv_key.clone_key())?;
            Some(TlsAcceptor::from(Arc::new(server_config)))
        } else {
            None
        };

        loop {
            let (stream, addr) = listener.accept().await?;
            info!(
                "New control connection from: {} (tls={})",
                addr,
                acceptor.is_some()
            );
            if let Err(_e) = set_keepalive(&stream) {
                error!("handle_single_client set_keepalive err");
                continue;
            }

            let active_clients_clone = self.active_clients.clone();
            let db_pool_clone = self.db_pool.clone();
            let redis_client_clone = self.redis_client.clone();
            let client_models = self.client_model.clone();
            let hot_models = self.hot_models.clone();
            let producer: Arc<FutureProducer> = self.producer.clone();
            let server_state_clone = self.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let streams: Result<(
                    Box<dyn AsyncRead + Send + Unpin>,
                    Box<dyn AsyncWrite + Send + Unpin>,
                )> = if let Some(acceptor) = acceptor {
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            let (reader, writer) = tokio::io::split(tls_stream);
                            Ok((Box::new(reader), Box::new(writer)))
                        }
                        Err(e) => Err(anyhow!("control TLS accept failed: {}", e)),
                    }
                } else {
                    let (reader, writer) = stream.into_split();
                    Ok((Box::new(reader), Box::new(writer)))
                };

                let (reader, writer) = match streams {
                    Ok(streams) => streams,
                    Err(e) => {
                        error!("Error preparing control stream {}: {}", addr, e);
                        return;
                    }
                };

                if let Err(e) = handle_single_client(
                    reader,
                    writer,
                    addr,
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
    mut reader: Box<dyn AsyncRead + Send + Unpin>,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    addr: std::net::SocketAddr,
    active_clients: ActiveClients,
    _client_models: Arc<ClientModelClass>,
    hot_models: Arc<HotModelClass>,
    db_pool: Arc<Pool<Postgres>>,
    producer: Arc<FutureProducer>,
    redis_client: Arc<RedisClient>,
    server_state: Arc<crate::handle::ServerState>,
) -> Result<()> {
    let writer = Arc::new(Mutex::new(writer));
    let connection_id = crate::handle::next_control_connection_id();

    let mut authed = false;
    let mut session_client_id = ClientId([0; 16]);
    let mut consumer_authed = false;
    let mut session_consumer_id = ClientId([0; 16]);
    let mut buf = BytesMut::with_capacity(1024 * 1024);

    loop {
        match read_command(&mut reader, &mut buf).await {
            Ok(Command::V1(CommandV1::Login {
                version,
                auto_models,
                client_id: id,
                os_type,
                system_info,
                device_memtotal_gb,
                device_total_tflops,
                devices_info,
            })) => {
                info!(
                    "Registration attempt for client {}",
                    ClientId(id).log_label()
                );
                debug!(
                    "Registration attempt for devices_info: {:?} device_total_tflops {}",
                    devices_info, device_total_tflops
                );

                let validate_result = match handle_login(
                    version,
                    auto_models,
                    &active_clients,
                    &redis_client,
                    &db_pool,
                    &hot_models,
                    &ClientId(id),
                    os_type,
                    devices_info.clone(),
                    geo::normalize_public_ip_from_device(
                        devices_info.first().map(|device| device.ip),
                        addr.ip(),
                    ),
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
                    connection_id,
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
                info!(
                    "Heartbeat received from client {}",
                    ClientId(id).log_label()
                );
                handle_heartbeat(
                    &db_pool,
                    &producer,
                    &ClientId(id),
                    geo::normalize_public_ip_from_device(
                        devices_info.first().map(|device| device.ip),
                        addr.ip(),
                    ),
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
                    ClientId(id).log_label(),
                    auto_models_device.len()
                );

                upsert_client_models_in_redis(&redis_client, &ClientId(id), &models).await;

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
                info!("addr {} disconnected: {}", addr, e);
                if authed {
                    let should_mark_offline = {
                        let mut clients = active_clients.lock().await;
                        match clients.get(&session_client_id) {
                            Some(info) if info.connection_id == connection_id => {
                                clients.remove(&session_client_id);
                                true
                            }
                            Some(info) => {
                                debug!(
                                    "Ignoring stale disconnect for client {} connection {}; current connection is {}",
                                    session_client_id.log_label(),
                                    connection_id,
                                    info.connection_id
                                );
                                false
                            }
                            None => false,
                        }
                    };

                    if should_mark_offline {
                        if let Err(status_err) =
                            client::upsert_client_status(&db_pool, &session_client_id, "offline")
                                .await
                        {
                            warn!(
                                "Failed to mark client {} offline: {}",
                                session_client_id.log_label(),
                                status_err
                            );
                        }
                    }
                } else if consumer_authed {
                    let should_remove_consumer = {
                        let mut consumers = server_state.consumer_sessions.lock().await;
                        match consumers.get(&session_consumer_id) {
                            Some(info) if info.connection_id == connection_id => {
                                consumers.remove(&session_consumer_id);
                                true
                            }
                            Some(info) => {
                                debug!(
                                    "Ignoring stale disconnect for consumer {} connection {}; current connection is {}",
                                    session_consumer_id.log_label(),
                                    connection_id,
                                    info.connection_id
                                );
                                false
                            }
                            None => false,
                        }
                    };

                    if should_remove_consumer {
                        info!(
                            "P2P consumer {} disconnected",
                            session_consumer_id.log_label()
                        );
                    }
                } else {
                    debug!("Unauthenticated control connection {} disconnected", addr);
                }
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
                    session_client_id.log_label()
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
                phase,
                done,
                error,
                prompt_tokens,
                completion_tokens,
                analysis_tokens,
                final_tokens,
            })) => {
                server_state
                    .inference_scheduler
                    .handle_inference_result_chunk(
                        task_id,
                        seq,
                        delta,
                        phase,
                        done,
                        error,
                        prompt_tokens,
                        completion_tokens,
                        analysis_tokens,
                        final_tokens,
                    )
                    .await;
            }
            Ok(Command::V1(CommandV1::EmbeddingResult {
                task_id,
                success,
                embeddings,
                error,
                prompt_tokens,
            })) => {
                server_state
                    .inference_scheduler
                    .handle_embedding_result(task_id, success, embeddings, error, prompt_tokens)
                    .await;
            }

            Ok(Command::V1(CommandV1::ModelDownloadProgress {
                client_id: id,
                model_name,
                downloaded_bytes,
                total_bytes,
                percentage,
                speed_bps,
                status,
                error,
            })) => {
                let is_noisy_pending = status == DownloadStatus::Pending
                    && downloaded_bytes == 0
                    && speed_bps == 0
                    && percentage <= 0.0
                    && error.is_none();

                if !is_noisy_pending {
                    info!(
                        "Model download progress from client {}: model={}, progress={:.1}%, downloaded={}/{}, speed={}/s, status={:?}, error_present={}",
                        ClientId(id).log_label(),
                        model_name,
                        percentage,
                        format_bytes!(downloaded_bytes),
                        format_bytes!(total_bytes),
                        format_bytes!(speed_bps),
                        status,
                        error.is_some()
                    );
                } else {
                    debug!(
                        "Model download progress from client {}: model={}, progress={:.1}%, downloaded={}/{}, speed={}/s, status={:?}, error_present={}",
                        ClientId(id).log_label(),
                        model_name,
                        percentage,
                        format_bytes!(downloaded_bytes),
                        format_bytes!(total_bytes),
                        format_bytes!(speed_bps),
                        status,
                        error.is_some()
                    );
                }

                // Store or delete progress in Redis
                update_model_download_progress_in_redis(
                    &redis_client,
                    &ClientId(id),
                    &model_name,
                    downloaded_bytes,
                    total_bytes,
                    percentage,
                    speed_bps,
                    &status,
                    error.as_deref(),
                )
                .await;
            }

            Ok(Command::V2(CommandV2::P2PConsumerLogin {
                consumer_id,
                api_token,
            })) => {
                if authed {
                    return Err(anyhow!("P2PConsumerLogin after device login"));
                }
                if consumer_authed {
                    return Err(anyhow!("P2PConsumerLogin repeated on same session"));
                }

                let token = api_token.into_inner();
                let login_result = handle_consumer_login(
                    &db_pool,
                    &server_state,
                    &ClientId(consumer_id),
                    token,
                    &writer,
                    connection_id,
                    &mut consumer_authed,
                )
                .await;

                let response = match login_result {
                    Ok(()) => {
                        session_consumer_id = ClientId(consumer_id);
                        CommandV2::P2PConsumerLoginResult {
                            success: true,
                            error: None,
                        }
                    }
                    Err(e) => {
                        warn!(
                            "P2P consumer {} login failed: {}",
                            ClientId(consumer_id).log_label(),
                            e
                        );
                        CommandV2::P2PConsumerLoginResult {
                            success: false,
                            error: Some(e.to_string()),
                        }
                    }
                };
                write_command(&mut *writer.lock().await, &Command::V2(response)).await?;
            }

            Ok(Command::V2(CommandV2::P2PConnectionRequest {
                source_client_id,
                target_client_id,
                connection_id,
            })) => {
                let source_id = ClientId(source_client_id);
                let target_id = ClientId(target_client_id);

                let (source_writer, target_writer, source_consumer_token_hash) = {
                    let clients = active_clients.lock().await;
                    let target = clients
                        .get(&target_id)
                        .map(|c| c.writer.clone())
                        .ok_or_else(|| anyhow!("Target client not online"))?;

                    let source = if authed {
                        if session_client_id != source_id {
                            return Err(anyhow!(
                                "P2PConnectionRequest source_client_id mismatch with device session"
                            ));
                        }
                        clients
                            .get(&source_id)
                            .map(|c| c.writer.clone())
                            .ok_or_else(|| anyhow!("Source client not online"))?
                    } else if consumer_authed {
                        if session_consumer_id != source_id {
                            return Err(anyhow!(
                                "P2PConnectionRequest source_client_id mismatch with consumer session"
                            ));
                        }
                        drop(clients);
                        let consumers = server_state.consumer_sessions.lock().await;
                        let consumer = consumers
                            .get(&source_id)
                            .ok_or_else(|| anyhow!("Source consumer not online"))?;
                        if !consumer.authed {
                            return Err(anyhow!("Source consumer not authenticated"));
                        }
                        if !consumer.allowed_client_ids.contains(&target_id) {
                            return Err(anyhow!("Target client is not allowed for this consumer"));
                        }
                        consumer.writer.clone()
                    } else {
                        return Err(anyhow!("P2PConnectionRequest before login"));
                    };

                    let source_consumer_token_hash = if consumer_authed {
                        let consumers = server_state.consumer_sessions.lock().await;
                        consumers.get(&source_id).map(|c| c.token_hash.clone())
                    } else {
                        None
                    };

                    (source, target, source_consumer_token_hash)
                };

                let turn_host =
                    std::env::var("TURN_HOST").map_err(|_| anyhow!("TURN_HOST env is required"))?;
                let _turn_port: u16 = std::env::var("TURN_TURNS_PORT")
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

                let mut data_plane_secret = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut data_plane_secret);

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
                    turn_password: RedactedString::from(password.clone()),
                    data_plane_secret: DataPlaneSecret(data_plane_secret),
                    expires_at,
                    force_tls: false,
                });

                let to_target = Command::V2(CommandV2::P2PConnectionConfig {
                    peer_id: source_client_id,
                    connection_id,
                    stun_urls,
                    turn_urls,
                    turn_username: username,
                    turn_password: RedactedString::from(password),
                    data_plane_secret: DataPlaneSecret(data_plane_secret),
                    expires_at,
                    force_tls: false,
                });

                if consumer_authed {
                    let mut sessions = server_state.p2p_usage_sessions.lock().await;
                    prune_p2p_usage_sessions(&mut sessions);
                    let conn_key = ClientId(connection_id);
                    if sessions.contains_key(&conn_key) {
                        return Err(anyhow!("Duplicate P2P connection_id"));
                    }
                    sessions.insert(
                        conn_key,
                        P2PUsageSession {
                            source_client_id: source_id,
                            target_client_id: target_id,
                            source_is_consumer: true,
                            consumer_token_hash: source_consumer_token_hash,
                            created_at: Utc::now(),
                            recording: false,
                            recorded: false,
                            consumer_report: None,
                            target_receipt: None,
                        },
                    );
                }

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
                if !authed && !consumer_authed {
                    return Err(anyhow!("P2PCandidates before login"));
                }

                let src = ClientId(source_client_id);
                let dst = ClientId(target_client_id);

                // Require that the sender matches the current session.
                let sender_matches_session = (authed && session_client_id == src)
                    || (consumer_authed && session_consumer_id == src);
                if !sender_matches_session {
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
                    if let Some(client) = clients.get(&dst) {
                        client.writer.clone()
                    } else {
                        drop(clients);
                        let consumers = server_state.consumer_sessions.lock().await;
                        consumers
                            .get(&dst)
                            .map(|c| c.writer.clone())
                            .ok_or_else(|| anyhow!("Target peer not online"))?
                    }
                };

                let forward = Command::V2(CommandV2::P2PCandidates {
                    source_client_id,
                    target_client_id,
                    connection_id,
                    candidates,
                });
                write_command(&mut *target_writer.lock().await, &forward).await?;
            }
            Ok(Command::V2(CommandV2::P2PUsageReport {
                consumer_id,
                target_client_id,
                connection_id,
                task_id,
                request_id,
                model,
                endpoint,
                transport,
                stream,
                multimodal,
                prompt_tokens,
                completion_tokens,
                total_tokens,
                analysis_tokens,
                final_tokens,
                bytes_up,
                bytes_down,
                chunk_count,
                retry_count,
                connect_ms,
                ttft_ms,
                total_ms,
                success,
                error,
                output_sha256,
            })) => {
                if !consumer_authed {
                    return Err(anyhow!("P2PUsageReport before consumer login"));
                }
                handle_p2p_usage_report(
                    &db_pool,
                    &server_state,
                    session_consumer_id,
                    ClientId(consumer_id),
                    ClientId(target_client_id),
                    ClientId(connection_id),
                    task_id,
                    request_id,
                    model,
                    endpoint,
                    transport,
                    stream,
                    multimodal,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                    analysis_tokens,
                    final_tokens,
                    bytes_up,
                    bytes_down,
                    chunk_count,
                    retry_count,
                    connect_ms,
                    ttft_ms,
                    total_ms,
                    success,
                    error,
                    output_sha256,
                )
                .await?;
            }
            Ok(Command::V2(CommandV2::P2PUsageReceipt {
                source_client_id,
                target_client_id,
                connection_id,
                task_id,
                prompt_tokens,
                completion_tokens,
                total_tokens,
                analysis_tokens,
                final_tokens,
                success,
                error,
                output_sha256,
            })) => {
                if !authed {
                    return Err(anyhow!("P2PUsageReceipt before device login"));
                }
                handle_p2p_usage_receipt(
                    &db_pool,
                    &server_state,
                    session_client_id,
                    ClientId(source_client_id),
                    ClientId(target_client_id),
                    ClientId(connection_id),
                    task_id,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                    analysis_tokens,
                    final_tokens,
                    success,
                    error,
                    output_sha256,
                )
                .await?;
            }
            _ => {
                warn!("Received unexpected command from client addr {}", addr);
            }
        }
    }
    #[allow(unreachable_code)]
    Ok(()) // This is theoretically unreachable but required by compiler
}

fn prune_p2p_usage_sessions(sessions: &mut HashMap<ClientId, P2PUsageSession>) {
    let now = Utc::now();
    sessions.retain(|_, session| {
        !session.recorded
            && now.signed_duration_since(session.created_at) <= chrono::Duration::hours(1)
    });
}

fn sanitize_usage_string(value: String, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(max_chars)
        .collect()
}

fn sanitize_optional_usage_string(value: Option<String>, max_chars: usize) -> Option<String> {
    value
        .map(|value| sanitize_usage_string(value, max_chars))
        .filter(|value| !value.is_empty())
}

fn normalize_p2p_endpoint(endpoint: String) -> Result<String> {
    let endpoint = sanitize_usage_string(endpoint, 64).to_ascii_lowercase();
    match endpoint.as_str() {
        "chat.completion" | "completion" | "embeddings" | "sophnet_embeddings" | "ocr"
        | "ocr.image" | "multimodal.chat" => Ok(endpoint),
        _ => Err(anyhow!("Unsupported P2P usage endpoint")),
    }
}

fn normalized_usage_total(prompt_tokens: u32, completion_tokens: u32, total_tokens: u32) -> u32 {
    if total_tokens == 0 {
        prompt_tokens.saturating_add(completion_tokens)
    } else {
        total_tokens
    }
}

fn p2p_usage_matches(
    report: &P2PConsumerUsageReport,
    receipt: &P2PTargetUsageReceipt,
) -> Result<()> {
    if report.task_id != receipt.task_id {
        return Err(anyhow!("P2P usage task_id mismatch"));
    }
    if report.success != receipt.success {
        return Err(anyhow!("P2P usage success mismatch"));
    }

    let report_total = normalized_usage_total(
        report.prompt_tokens,
        report.completion_tokens,
        report.total_tokens,
    );
    let receipt_total = normalized_usage_total(
        receipt.prompt_tokens,
        receipt.completion_tokens,
        receipt.total_tokens,
    );

    if report.success {
        if report.output_sha256.is_none() || receipt.output_sha256.is_none() {
            return Err(anyhow!(
                "P2P success usage requires output hash from both peers"
            ));
        }
        if report.output_sha256 != receipt.output_sha256 {
            return Err(anyhow!("P2P usage output hash mismatch"));
        }
        if report.prompt_tokens != receipt.prompt_tokens
            || report.completion_tokens != receipt.completion_tokens
            || report_total != receipt_total
            || report.analysis_tokens != receipt.analysis_tokens
            || report.final_tokens != receipt.final_tokens
        {
            return Err(anyhow!("P2P usage token counts mismatch"));
        }
    } else if report.error.as_deref() != receipt.error.as_deref() {
        debug!(
            "P2P failure usage errors differ: consumer_error_present={} target_error_present={}",
            report.error.is_some(),
            receipt.error.is_some()
        );
    }

    Ok(())
}

fn finalize_p2p_usage_if_ready(session: &mut P2PUsageSession) -> Result<Option<TokenUsageInsert>> {
    if session.recorded || session.recording {
        return Ok(None);
    }
    if !session.source_is_consumer {
        return Ok(None);
    }

    let (Some(report), Some(receipt)) = (&session.consumer_report, &session.target_receipt) else {
        return Ok(None);
    };

    if let Err(err) = p2p_usage_matches(report, receipt) {
        session.recorded = true;
        return Err(err);
    }

    let token_hash = session
        .consumer_token_hash
        .clone()
        .ok_or_else(|| anyhow!("P2P usage session missing consumer token hash"))?;
    let total_tokens = normalized_usage_total(
        report.prompt_tokens,
        report.completion_tokens,
        report.total_tokens,
    );
    if report.success && total_tokens == 0 {
        session.recorded = true;
        return Err(anyhow!("P2P success usage has zero tokens"));
    }

    session.recording = true;
    Ok(Some(TokenUsageInsert {
        request_id: report.request_id.clone(),
        token_hash: Some(token_hash),
        client_id: session.target_client_id,
        model: report.model.clone(),
        endpoint: report.endpoint.clone(),
        prompt_tokens: report.prompt_tokens,
        completion_tokens: report.completion_tokens,
        success: report.success,
        stream: report.stream,
    }))
}

async fn insert_finalized_p2p_usage(
    db_pool: &Arc<Pool<Postgres>>,
    server_state: &Arc<crate::handle::ServerState>,
    connection_id: ClientId,
    usage: Option<TokenUsageInsert>,
) -> Result<()> {
    let Some(usage) = usage else {
        return Ok(());
    };
    if let Err(err) = insert_token_usage(db_pool, usage).await {
        let mut sessions = server_state.p2p_usage_sessions.lock().await;
        if let Some(session) = sessions.get_mut(&connection_id) {
            session.recording = false;
        }
        return Err(err);
    }
    let mut sessions = server_state.p2p_usage_sessions.lock().await;
    if let Some(session) = sessions.get_mut(&connection_id) {
        session.recording = false;
        session.recorded = true;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_p2p_usage_report(
    db_pool: &Arc<Pool<Postgres>>,
    server_state: &Arc<crate::handle::ServerState>,
    session_consumer_id: ClientId,
    consumer_id: ClientId,
    target_client_id: ClientId,
    connection_id: ClientId,
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
) -> Result<()> {
    if session_consumer_id != consumer_id {
        return Err(anyhow!("P2PUsageReport consumer_id mismatch with session"));
    }
    if matches!(transport, P2PUsageTransport::FallbackHttp) {
        return Err(anyhow!(
            "Fallback HTTP usage must be recorded by the HTTP gateway"
        ));
    }

    let consumer_token_hash = {
        let consumers = server_state.consumer_sessions.lock().await;
        let consumer = consumers
            .get(&consumer_id)
            .ok_or_else(|| anyhow!("P2P consumer session not found"))?;
        if !consumer.authed {
            return Err(anyhow!("P2P consumer session is not authenticated"));
        }
        if !consumer.allowed_client_ids.contains(&target_client_id) {
            return Err(anyhow!("P2P usage target is not allowed for this consumer"));
        }
        consumer.token_hash.clone()
    };

    let report = P2PConsumerUsageReport {
        task_id: sanitize_usage_string(task_id, 128),
        request_id: sanitize_optional_usage_string(request_id, 128),
        model: sanitize_usage_string(model, 128),
        endpoint: normalize_p2p_endpoint(endpoint)?,
        stream,
        multimodal,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        analysis_tokens,
        final_tokens,
        success,
        error: sanitize_optional_usage_string(error, 512),
        output_sha256,
    };

    debug!(
        "P2P usage report consumer={} target={} conn={} endpoint={} stream={} multimodal={} transport={:?} bytes_up={} bytes_down={} chunks={} retries={} connect_ms={} ttft_ms={:?} total_ms={} success={}",
        consumer_id.log_label(),
        target_client_id.log_label(),
        connection_id.log_label(),
        report.endpoint,
        report.stream,
        report.multimodal,
        transport,
        bytes_up,
        bytes_down,
        chunk_count,
        retry_count,
        connect_ms,
        ttft_ms,
        total_ms,
        report.success
    );

    let usage = {
        let mut sessions = server_state.p2p_usage_sessions.lock().await;
        prune_p2p_usage_sessions(&mut sessions);
        let session = sessions
            .get_mut(&connection_id)
            .ok_or_else(|| anyhow!("Unknown P2P usage connection_id"))?;
        if session.source_client_id != consumer_id || session.target_client_id != target_client_id {
            session.recorded = true;
            return Err(anyhow!("P2PUsageReport connection ownership mismatch"));
        }
        if !session.source_is_consumer {
            return Err(anyhow!(
                "P2PUsageReport is only accepted for consumer sessions"
            ));
        }
        match &session.consumer_token_hash {
            Some(token_hash) if token_hash == &consumer_token_hash => {}
            Some(_) => {
                session.recorded = true;
                return Err(anyhow!("P2PUsageReport token binding mismatch"));
            }
            None => session.consumer_token_hash = Some(consumer_token_hash),
        }
        session.consumer_report = Some(report);
        finalize_p2p_usage_if_ready(session)?
    };

    insert_finalized_p2p_usage(db_pool, server_state, connection_id, usage).await
}

#[allow(clippy::too_many_arguments)]
async fn handle_p2p_usage_receipt(
    db_pool: &Arc<Pool<Postgres>>,
    server_state: &Arc<crate::handle::ServerState>,
    session_client_id: ClientId,
    source_client_id: ClientId,
    target_client_id: ClientId,
    connection_id: ClientId,
    task_id: String,
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    analysis_tokens: u32,
    final_tokens: u32,
    success: bool,
    error: Option<String>,
    output_sha256: Option<[u8; 32]>,
) -> Result<()> {
    if session_client_id != target_client_id {
        return Err(anyhow!(
            "P2PUsageReceipt target_client_id mismatch with device session"
        ));
    }

    let receipt = P2PTargetUsageReceipt {
        task_id: sanitize_usage_string(task_id, 128),
        prompt_tokens,
        completion_tokens,
        total_tokens,
        analysis_tokens,
        final_tokens,
        success,
        error: sanitize_optional_usage_string(error, 512),
        output_sha256,
    };

    debug!(
        "P2P usage receipt source={} target={} conn={} success={}",
        source_client_id.log_label(),
        target_client_id.log_label(),
        connection_id.log_label(),
        receipt.success
    );

    let usage = {
        let mut sessions = server_state.p2p_usage_sessions.lock().await;
        prune_p2p_usage_sessions(&mut sessions);
        let session = sessions
            .get_mut(&connection_id)
            .ok_or_else(|| anyhow!("Unknown P2P usage receipt connection_id"))?;
        if session.source_client_id != source_client_id
            || session.target_client_id != target_client_id
        {
            session.recorded = true;
            return Err(anyhow!("P2PUsageReceipt connection ownership mismatch"));
        }
        session.target_receipt = Some(receipt);
        finalize_p2p_usage_if_ready(session)?
    };

    insert_finalized_p2p_usage(db_pool, server_state, connection_id, usage).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_pair() -> (P2PConsumerUsageReport, P2PTargetUsageReceipt) {
        let output_sha256 = Some([7u8; 32]);
        (
            P2PConsumerUsageReport {
                task_id: "task-1".to_string(),
                request_id: Some("req-1".to_string()),
                model: "gpuf".to_string(),
                endpoint: "chat.completion".to_string(),
                stream: false,
                multimodal: false,
                prompt_tokens: 3,
                completion_tokens: 2,
                total_tokens: 5,
                analysis_tokens: 0,
                final_tokens: 2,
                success: true,
                error: None,
                output_sha256,
            },
            P2PTargetUsageReceipt {
                task_id: "task-1".to_string(),
                prompt_tokens: 3,
                completion_tokens: 2,
                total_tokens: 5,
                analysis_tokens: 0,
                final_tokens: 2,
                success: true,
                error: None,
                output_sha256,
            },
        )
    }

    #[test]
    fn p2p_usage_requires_matching_receipt() {
        let (report, receipt) = usage_pair();
        assert!(p2p_usage_matches(&report, &receipt).is_ok());
    }

    #[test]
    fn p2p_usage_rejects_output_hash_mismatch() {
        let (report, mut receipt) = usage_pair();
        receipt.output_sha256 = Some([8u8; 32]);
        assert!(p2p_usage_matches(&report, &receipt).is_err());
    }
}

async fn handle_login(
    version: u32,
    auto_models: bool,
    active_clients: &Arc<Mutex<HashMap<ClientId, ClientInfo>>>,
    redis_client: &Arc<RedisClient>,
    db_pool: &Pool<Postgres>,
    hot_models: &Arc<HotModelClass>,
    client_id: &ClientId,
    os_type: OsType,
    devices_info: Vec<DevicesInfo>,
    public_ip: String,
    system_info: SystemInfo,
    writer: &Arc<Mutex<ControlWriter>>,
    connection_id: crate::handle::ConnectionId,
    authed: &mut bool,
) -> Result<CommandV1> {
    info!("Registration attempt for client {}", client_id.log_label());
    let mut clients = active_clients.lock().await;
    if let Some(existing) = clients.get(client_id) {
        warn!(
            "Client {} already registered on connection {}, replacing with new connection {}.",
            client_id.log_label(),
            existing.connection_id,
            connection_id
        );
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
        info!("Client {} registered successfully", client_id.log_label());
        *authed = true;

        if let Err(e) = client::mark_client_online_seen(db_pool, client_id).await {
            warn!(
                "Failed to mark client {} online on login: {}",
                client_id.log_label(),
                e
            );
        }

        let geo_location = geo::lookup_geo(&public_ip).await;
        if let Err(e) =
            client::update_client_network_geo(db_pool, client_id, &public_ip, &geo_location).await
        {
            warn!("Failed to update client network geo: {}", e);
        }

        // Only recommend models if auto_models is enabled
        let pods_model = if auto_models {
            models::get_models_batch(&hot_models, &devices_info).await?
        } else {
            Vec::new()
        };

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
        "Client {} login result success={} pod_count={}",
        client_id.log_label(),
        matches!(
            validate_result,
            CommandV1::LoginResult { success: true, .. }
        ),
        match &validate_result {
            CommandV1::LoginResult { pods_model, .. } => pods_model.len(),
            _ => 0,
        }
    );

    if *authed {
        clients.insert(
            *client_id,
            ClientInfo {
                connection_id,
                writer: writer.clone(),
                authed: true,
                version,
                os_type,
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
    }
    Ok(validate_result)
}

async fn handle_consumer_login(
    db_pool: &Arc<Pool<Postgres>>,
    server_state: &Arc<crate::handle::ServerState>,
    consumer_id: &ClientId,
    api_token: String,
    writer: &Arc<Mutex<ControlWriter>>,
    connection_id: crate::handle::ConnectionId,
    authed: &mut bool,
) -> Result<()> {
    let token = api_token.trim();
    if token.is_empty() {
        return Err(anyhow!("Missing API token"));
    }
    if token.len() != 48 {
        return Err(anyhow!("Invalid API token length"));
    }

    let (allowed_client_ids, _access_level) =
        client::get_user_client_by_token(db_pool, token).await?;
    let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

    {
        let clients = server_state.active_clients.lock().await;
        if clients.contains_key(consumer_id) {
            return Err(anyhow!(
                "P2P consumer id conflicts with an online compute client"
            ));
        }
    }

    let mut consumers = server_state.consumer_sessions.lock().await;
    if let Some(existing) = consumers.get(consumer_id) {
        warn!(
            "P2P consumer {} already registered on connection {}, replacing with new connection {}.",
            consumer_id.log_label(),
            existing.connection_id,
            connection_id
        );
    }

    *authed = true;
    consumers.insert(
        *consumer_id,
        ConsumerSession {
            connection_id,
            writer: writer.clone(),
            authed: true,
            allowed_client_ids,
            token_hash,
            connected_at: Utc::now(),
        },
    );

    info!(
        "P2P consumer {} registered successfully",
        consumer_id.log_label()
    );
    Ok(())
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
            .get_hot_model_with_details(device.memtotal_gb as u32, device.engine_type.to_i16())
            .await
        {
            Ok(model_info) => {
                pods_model.push(PodModel {
                    pod_id: device.pod_id,
                    model_name: if model_info.name.is_empty() {
                        None
                    } else {
                        Some(model_info.name)
                    },
                    download_url: model_info.download_url,
                    checksum: model_info.checksum,
                    expected_size: model_info.expected_size.map(|s| s as u64),
                });
            }
            Err(e) => {
                pods_model.push(PodModel {
                    pod_id: device.pod_id,
                    model_name: None,
                    download_url: None,
                    checksum: None,
                    expected_size: None,
                });
                error!("Failed to get hot model: {}", e);
            }
        };
    }

    Ok(pods_model)
}

async fn upsert_client_models_in_redis(
    redis_client: &Arc<RedisClient>,
    client_id: &ClientId,
    models: &[Model],
) {
    let Ok(mut conn) = redis_client.get_async_connection().await else {
        return;
    };

    let key = format!("client:{}:models", client_id);
    let payload = match serde_json::to_string(models) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to serialize client models to JSON: {}", e);
            return;
        }
    };

    // Keep this fairly short so it's "realtime".
    let _: std::result::Result<(), _> = conn.set(&key, payload).await;
    let _: std::result::Result<(), _> = conn.expire(&key, 300).await;
}

async fn handle_heartbeat(
    db_pool: &Pool<Postgres>,
    producer: &Arc<FutureProducer>,
    client_id: &ClientId,
    public_ip: String,
    system_info: common::SystemInfo,
    devices_info: Vec<common::DevicesInfo>,
    device_memtotal_gb: u32,
    device_count: u32,
    total_tflops: u32,
) {
    debug!("Sending heartbeat to consumer client {} cpu_usage {}% memory_usage {}% disk_usage {}% device_memtotal_gb {} GB device_count {} total_tflops {} tflops", client_id.log_label(), system_info.cpu_usage, system_info.memory_usage, system_info.disk_usage, device_memtotal_gb, device_count, total_tflops);

    if let Err(e) = client::mark_client_online_seen(db_pool, client_id).await {
        warn!(
            "Failed to refresh client {} online status on heartbeat: {}",
            client_id.log_label(),
            e
        );
    }

    let geo_location = geo::lookup_geo(&public_ip).await;
    if let Err(e) =
        client::update_client_network_geo(db_pool, client_id, &public_ip, &geo_location).await
    {
        warn!("Failed to update client heartbeat network geo: {}", e);
    }

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

/// Update model download progress in Redis
/// Simplified version: one key per client, 60 seconds TTL
/// If download is completed, delete the key; otherwise, update with current progress
async fn update_model_download_progress_in_redis(
    redis_client: &Arc<RedisClient>,
    client_id: &ClientId,
    model_name: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    percentage: f32,
    speed_bps: u64,
    status: &common::DownloadStatus,
    error: Option<&str>,
) {
    use redis::AsyncCommands;

    let Ok(mut conn) = redis_client.get_async_connection().await else {
        error!("Failed to get Redis connection for model download progress");
        return;
    };

    // Simplified key format: one key per client
    let key = format!("client:{}:model_download", client_id);

    // If download is completed or failed, delete the key
    if matches!(
        status,
        common::DownloadStatus::Completed | common::DownloadStatus::Failed
    ) {
        if let Err(e) = conn.del::<_, ()>(&key).await {
            error!("Failed to delete model download progress from Redis: {}", e);
        } else {
            info!(
                "Deleted model download progress from Redis for client {}",
                client_id.log_label()
            );
        }
        return;
    }

    // Otherwise, update the progress
    let timestamp = chrono::Utc::now().timestamp();
    let status_str = format!("{:?}", status);

    let mut fields: Vec<(&str, String)> = vec![
        ("model_name", model_name.to_string()),
        ("downloaded_bytes", downloaded_bytes.to_string()),
        ("total_bytes", total_bytes.to_string()),
        ("percentage", format!("{:.2}", percentage)),
        ("speed_bps", speed_bps.to_string()),
        ("status", status_str),
        ("timestamp", timestamp.to_string()),
    ];

    if let Some(err) = error {
        fields.push(("error", err.to_string()));
    }

    if let Err(e) = conn.hset_multiple::<_, _, _, ()>(&key, &fields).await {
        error!("Failed to update model download progress in Redis: {}", e);
    } else {
        // Set expiration to 60 seconds for auto-cleanup
        let _: Result<(), _> = conn.expire(&key, 60).await;
    }
}
