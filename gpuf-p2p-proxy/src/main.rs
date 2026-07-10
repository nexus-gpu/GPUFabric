use anyhow::{anyhow, Context, Result};
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bincode::config as bincode_config;
use clap::Parser;
use common::{
    read_command, write_command, ChatContentPart, ChatMessageContent, Command, CommandV2,
    P2PCandidate, P2PCandidateType, P2PTransport, P2PUsageTransport, RedactedString,
    MAX_MESSAGE_SIZE,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::mpsc,
    time::timeout,
};
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, ServerName},
        ClientConfig, RootCertStore,
    },
    TlsConnector,
};
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};

const P2P_UDP_MAGIC: [u8; 4] = *b"P2PU";
const P2P_UDP_VERSION: u8 = 2;
const P2P_UDP_FLAG_ACK: u8 = 0x01;
const P2P_UDP_HEADER_LEN: usize = 4 + 1 + 1 + 4 + 2 + 2 + 8 + 32;
const P2P_UDP_MTU_PAYLOAD: usize = 1200;
const P2P_MAX_FRAGMENTS_PER_MESSAGE: usize = 128;
const P2P_REPLAY_WINDOW_SECS: u64 = 300;

trait ControlIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> ControlIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "Lightweight GPUFabric P2P API proxy")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:18088")]
    listen: SocketAddr,

    #[arg(long, default_value = "127.0.0.1")]
    server_addr: String,

    #[arg(long, default_value_t = 17000)]
    control_port: u16,

    #[arg(long, alias = "source-client-id", env = "GPUF_P2P_PROXY_CONSUMER_ID")]
    consumer_id: String,

    #[arg(long, env = "GPUF_P2P_PROXY_TARGET_CLIENT_ID")]
    target_client_id: Option<String>,

    #[arg(long, env = "GPUF_P2P_PROXY_FALLBACK_BASE_URL")]
    fallback_base_url: Option<String>,

    #[arg(long, default_value_t = false)]
    control_tls: bool,

    #[arg(long)]
    control_tls_server_name: Option<String>,

    #[arg(long, default_value = "ca-cert.pem")]
    cert_chain_path: String,

    #[arg(long, default_value_t = 6)]
    p2p_connect_timeout_secs: u64,

    #[arg(long, default_value_t = 60)]
    p2p_response_timeout_secs: u64,

    #[arg(long, default_value_t = false)]
    disable_p2p: bool,
}

#[derive(Clone)]
struct AppState {
    args: Arc<Args>,
    consumer_id: [u8; 16],
    fallback: Option<FallbackClient>,
}

#[derive(Clone)]
struct FallbackClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize, Clone)]
struct ChatCompletionRequest {
    model: Option<String>,
    messages: Vec<ChatMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_k: Option<u32>,
    top_p: Option<f32>,
    repeat_penalty: Option<f32>,
    repeat_last_n: Option<i32>,
    min_keep: Option<u32>,
    stream: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ChatMessage {
    role: String,
    content: ChatMessageContent,
}

#[derive(Debug)]
struct P2PTextResult {
    connection_id: [u8; 16],
    task_id: String,
    text: String,
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
    output_sha256: [u8; 32],
}

#[derive(Debug, Serialize)]
struct OpenAIChatResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    client_id: String,
    p2p: OpenAIP2PInfo,
    choices: Vec<OpenAIChatChoice>,
    usage: OpenAIUsage,
}

#[derive(Debug, Serialize)]
struct OpenAIP2PInfo {
    enabled: bool,
    transport: &'static str,
    fallback: bool,
}

#[derive(Debug, Serialize)]
struct OpenAIChatChoice {
    index: i32,
    message: OpenAIChatMessage,
    finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct OpenAIChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    analysis_tokens: u32,
    final_tokens: u32,
}

#[derive(Debug, Serialize)]
struct OpenAIChatStreamChunk {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    client_id: String,
    p2p: OpenAIP2PInfo,
    choices: Vec<OpenAIChatStreamChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Serialize)]
struct OpenAIChatStreamChoice {
    index: i32,
    delta: Value,
    finish_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
enum P2PInferencePayload {
    TextPrompt(String),
    ChatMessages(Vec<ChatMessage>),
}

#[derive(Debug, Default, Clone, Copy)]
struct P2PUdpSendStats {
    bytes_sent: u64,
    retry_count: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gpuf_p2p_proxy=info,tower_http=info".into()),
        )
        .init();

    let args = Arc::new(Args::parse());
    let consumer_id = parse_client_id_hex(&args.consumer_id)?;
    let fallback = args
        .fallback_base_url
        .as_ref()
        .map(|base_url| FallbackClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        });

    let state = AppState {
        args: Arc::clone(&args),
        consumer_id,
        fallback,
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/completions", post(handle_fallback_post))
        .route("/v1/embeddings", post(handle_fallback_post))
        .route(
            "/api/open-apis/projects/:project_id/easyllms/embeddings",
            post(handle_fallback_post),
        )
        .route("/v1/models", get(handle_fallback_get))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind(args.listen).await?;
    info!("gpuf-p2p-proxy listening on http://{}", args.listen);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

async fn handle_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let request: ChatCompletionRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")),
    };

    let target_client_id = match target_client_id_from_headers(&headers)
        .or_else(|| state.args.target_client_id.clone())
    {
        Some(value) => match parse_client_id_hex(&value) {
            Ok(id) => id,
            Err(e) => return json_error(StatusCode::BAD_REQUEST, e.to_string()),
        },
        None => {
            return fallback_or_error(
                &state,
                Method::POST,
                "/v1/chat/completions",
                headers,
                body,
                StatusCode::BAD_REQUEST,
                "missing target client id; set --target-client-id or x-target-client-id",
            )
            .await;
        }
    };

    if state.args.disable_p2p {
        return fallback_or_error(
            &state,
            Method::POST,
            "/v1/chat/completions",
            headers,
            body,
            StatusCode::BAD_GATEWAY,
            "P2P is disabled",
        )
        .await;
    }

    let payload = match chat_request_to_payload(&request) {
        Ok(payload) => payload,
        Err(e) => {
            return fallback_or_error(
                &state,
                Method::POST,
                "/v1/chat/completions",
                headers,
                body,
                StatusCode::NOT_IMPLEMENTED,
                &format!("P2P text prompt conversion skipped: {e}"),
            )
            .await;
        }
    };

    let Some(api_token) = bearer_token_from_headers(&headers) else {
        return fallback_or_error(
            &state,
            Method::POST,
            "/v1/chat/completions",
            headers,
            body,
            StatusCode::UNAUTHORIZED,
            "missing Authorization bearer token for P2P consumer login",
        )
        .await;
    };

    let request_id = request_id_from_headers(&headers);
    let model = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
    if request.stream.unwrap_or(false) {
        let p2p = openai_chat_stream_response(
            state.clone(),
            target_client_id,
            request.clone(),
            payload,
            api_token,
            request_id,
        )
        .await;
        return match p2p {
            Ok(response) => response,
            Err(e) => {
                warn!("P2P stream inference failed before response, falling back when configured: {e:#}");
                fallback_or_error(
                    &state,
                    Method::POST,
                    "/v1/chat/completions",
                    headers,
                    body,
                    StatusCode::BAD_GATEWAY,
                    &format!("P2P stream inference failed: {e}"),
                )
                .await
            }
        };
    }

    let p2p = run_p2p_text_inference(
        &state,
        target_client_id,
        &request,
        payload,
        api_token,
        request_id,
    )
    .await;
    match p2p {
        Ok(result) => openai_chat_response(model, target_client_id, result),
        Err(e) => {
            warn!("P2P inference failed, falling back when configured: {e:#}");
            fallback_or_error(
                &state,
                Method::POST,
                "/v1/chat/completions",
                headers,
                body,
                StatusCode::BAD_GATEWAY,
                &format!("P2P inference failed: {e}"),
            )
            .await
        }
    }
}

async fn handle_fallback_post(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response<Body> {
    forward_fallback_request(&state, request).await
}

async fn handle_fallback_get(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response<Body> {
    forward_fallback_request(&state, request).await
}

async fn forward_fallback_request(state: &AppState, request: Request<Body>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map(|v| v.as_str().to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());
    let body = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(body) => body,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, format!("failed to read body: {e}")),
    };
    fallback_or_error(
        state,
        parts.method,
        &path,
        parts.headers,
        body,
        StatusCode::BAD_GATEWAY,
        "fallback gateway is not configured",
    )
    .await
}

async fn fallback_or_error(
    state: &AppState,
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: Bytes,
    status: StatusCode,
    message: &str,
) -> Response<Body> {
    match &state.fallback {
        Some(fallback) => match fallback.forward(method, path, headers, body).await {
            Ok(response) => response,
            Err(e) => json_error(StatusCode::BAD_GATEWAY, format!("fallback failed: {e}")),
        },
        None => json_error(status, message.to_string()),
    }
}

impl FallbackClient {
    async fn forward(
        &self,
        method: Method,
        path: &str,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<Response<Body>> {
        let url = format!("{}{}", self.base_url, path);
        let mut builder = self.client.request(method, url);
        for (name, value) in headers.iter() {
            if is_hop_by_hop_header(name) || name.as_str().eq_ignore_ascii_case("host") {
                continue;
            }
            builder = builder.header(name, value);
        }
        let response = builder.body(body).send().await?;
        let status = response.status();
        let mut out = Response::builder().status(status);
        for (name, value) in response.headers() {
            if is_hop_by_hop_header(name) {
                continue;
            }
            out = out.header(name, value);
        }
        let stream = response.bytes_stream();
        Ok(out.body(Body::from_stream(stream))?)
    }
}

fn is_hop_by_hop_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn target_client_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-target-client-id")
        .or_else(|| headers.get("x-gpuf-target-client-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn bearer_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn request_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("request-id")
        .or_else(|| headers.get("x-request-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .chars()
                .filter(|ch| !ch.is_control())
                .take(128)
                .collect::<String>()
        })
}

fn chat_request_to_payload(request: &ChatCompletionRequest) -> Result<P2PInferencePayload> {
    if request.messages.is_empty() {
        return Err(anyhow!("messages is empty"));
    }
    if request.messages.iter().any(chat_message_is_multimodal) {
        return Ok(P2PInferencePayload::ChatMessages(request.messages.clone()));
    }

    let mut prompt = String::new();
    for message in &request.messages {
        let text = match &message.content {
            ChatMessageContent::Text(text) => text.clone(),
            ChatMessageContent::Parts(parts) => parts_to_text(parts)?,
        };
        if text.trim().is_empty() {
            continue;
        }
        prompt.push_str(&message.role);
        prompt.push_str(": ");
        prompt.push_str(&text);
        prompt.push('\n');
    }
    prompt.push_str("assistant: ");
    Ok(P2PInferencePayload::TextPrompt(prompt))
}

fn p2p_inference_command(
    connection_id: [u8; 16],
    task_id: &str,
    request: &ChatCompletionRequest,
    payload: P2PInferencePayload,
) -> Command {
    match payload {
        P2PInferencePayload::TextPrompt(prompt) => Command::V2(CommandV2::P2PInferenceRequest {
            connection_id,
            task_id: task_id.to_string(),
            model: request.model.clone(),
            prompt,
            max_tokens: request.max_tokens.unwrap_or(1024),
            temperature: request.temperature.unwrap_or(0.7),
            top_k: request.top_k.unwrap_or(40),
            top_p: request.top_p.unwrap_or(0.9),
            repeat_penalty: request.repeat_penalty.unwrap_or(1.1),
            repeat_last_n: request.repeat_last_n.unwrap_or(64),
            min_keep: request.min_keep.unwrap_or(0),
        }),
        P2PInferencePayload::ChatMessages(messages) => Command::V2(CommandV2::ChatInferenceTask {
            task_id: task_id.to_string(),
            session_id: None,
            cache_policy: None,
            model: request.model.clone().unwrap_or_else(|| "gpuf".to_string()),
            messages: messages
                .into_iter()
                .map(|message| common::ChatMessageV2 {
                    role: message.role,
                    content: message.content,
                })
                .collect(),
            max_tokens: request.max_tokens.unwrap_or(1024),
            temperature: request.temperature.unwrap_or(0.7),
            top_k: request.top_k.unwrap_or(40),
            top_p: request.top_p.unwrap_or(0.9),
            repeat_penalty: request.repeat_penalty.unwrap_or(1.1),
            repeat_last_n: request.repeat_last_n.unwrap_or(64),
            min_keep: request.min_keep.unwrap_or(0),
        }),
    }
}

fn chat_message_is_multimodal(message: &ChatMessage) -> bool {
    matches!(&message.content, ChatMessageContent::Parts(parts) if parts.iter().any(ChatContentPart::is_image_like))
}

fn request_is_multimodal(request: &ChatCompletionRequest) -> bool {
    request.messages.iter().any(chat_message_is_multimodal)
}

fn usage_endpoint_for_request(request: &ChatCompletionRequest) -> &'static str {
    if request_is_multimodal(request) {
        let model = request
            .model
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if model.contains("ocr") {
            "ocr.image"
        } else {
            "multimodal.chat"
        }
    } else {
        "chat.completion"
    }
}

fn parts_to_text(parts: &[ChatContentPart]) -> Result<String> {
    let mut text = String::new();
    for part in parts {
        if part.is_image_like() {
            return Err(anyhow!(
                "multimodal image content requires P2P file transfer"
            ));
        }
        if part.r#type == "text" {
            if let Some(value) = part.text.as_deref() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(value);
            }
        }
    }
    Ok(text)
}

async fn run_p2p_text_inference(
    state: &AppState,
    target_client_id: [u8; 16],
    request: &ChatCompletionRequest,
    payload: P2PInferencePayload,
    api_token: String,
    request_id: Option<String>,
) -> Result<P2PTextResult> {
    let started_at = Instant::now();
    let connection_id = *uuid::Uuid::new_v4().as_bytes();
    let task_id = uuid::Uuid::new_v4().to_string();
    let mut control = connect_and_login(state, api_token).await?;

    let req = Command::V2(CommandV2::P2PConnectionRequest {
        source_client_id: state.consumer_id,
        target_client_id,
        connection_id,
    });
    write_command(&mut control, &req).await?;
    control.flush().await?;

    let (data_plane_secret, peer_candidates) = wait_for_p2p_config(
        &mut control,
        connection_id,
        state.args.p2p_connect_timeout_secs,
    )
    .await?;

    let peer_addr = select_udp_peer(&peer_candidates)?;
    debug!("selected P2P peer candidate {}", peer_addr);

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let mut bytes_up = 0u64;
    let mut retry_count = 0u32;
    let inf = p2p_inference_command(connection_id, &task_id, request, payload);
    let payload = udp_encode_command(&inf)?;
    let mut next_msg_id = 1u32;

    let handshake_stats = p2p_udp_send_reliable(
        &socket,
        peer_addr,
        connection_id,
        data_plane_secret,
        next_msg_id,
        &[],
    )
    .await
    .context("P2P handshake failed")?;
    bytes_up = bytes_up.saturating_add(handshake_stats.bytes_sent);
    retry_count = retry_count.saturating_add(handshake_stats.retry_count);
    next_msg_id = next_msg_id.wrapping_add(1);

    let request_stats = p2p_udp_send_reliable(
        &socket,
        peer_addr,
        connection_id,
        data_plane_secret,
        next_msg_id,
        &payload,
    )
    .await
    .context("P2P inference request send failed")?;
    bytes_up = bytes_up.saturating_add(request_stats.bytes_sent);
    retry_count = retry_count.saturating_add(request_stats.retry_count);
    let connect_ms = duration_ms(started_at.elapsed());

    let mut result = receive_p2p_text_result(
        &socket,
        peer_addr,
        connection_id,
        data_plane_secret,
        &task_id,
        state.args.p2p_response_timeout_secs,
        started_at,
    )
    .await?;
    result.bytes_up = result.bytes_up.saturating_add(bytes_up);
    result.retry_count = result.retry_count.saturating_add(retry_count);
    result.connect_ms = connect_ms;

    report_p2p_usage(
        &mut control,
        state.consumer_id,
        target_client_id,
        request,
        request_id,
        &result,
    )
    .await
    .context("failed to report P2P usage")?;

    Ok(result)
}

async fn openai_chat_stream_response(
    state: AppState,
    target_client_id: [u8; 16],
    request: ChatCompletionRequest,
    payload: P2PInferencePayload,
    api_token: String,
    request_id: Option<String>,
) -> Result<Response<Body>> {
    let started_at = Instant::now();
    let connection_id = *uuid::Uuid::new_v4().as_bytes();
    let task_id = uuid::Uuid::new_v4().to_string();
    let mut control = connect_and_login(&state, api_token).await?;

    let req = Command::V2(CommandV2::P2PConnectionRequest {
        source_client_id: state.consumer_id,
        target_client_id,
        connection_id,
    });
    write_command(&mut control, &req).await?;
    control.flush().await?;

    let (data_plane_secret, peer_candidates) = wait_for_p2p_config(
        &mut control,
        connection_id,
        state.args.p2p_connect_timeout_secs,
    )
    .await?;
    let peer_addr = select_udp_peer(&peer_candidates)?;
    debug!("selected P2P stream peer candidate {}", peer_addr);

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let inf = p2p_inference_command(connection_id, &task_id, &request, payload);
    let payload = udp_encode_command(&inf)?;
    let mut next_msg_id = 1u32;
    let mut bytes_up = 0u64;
    let mut retry_count = 0u32;

    let handshake_stats = p2p_udp_send_reliable(
        &socket,
        peer_addr,
        connection_id,
        data_plane_secret,
        next_msg_id,
        &[],
    )
    .await
    .context("P2P stream handshake failed")?;
    bytes_up = bytes_up.saturating_add(handshake_stats.bytes_sent);
    retry_count = retry_count.saturating_add(handshake_stats.retry_count);
    next_msg_id = next_msg_id.wrapping_add(1);

    let request_stats = p2p_udp_send_reliable(
        &socket,
        peer_addr,
        connection_id,
        data_plane_secret,
        next_msg_id,
        &payload,
    )
    .await
    .context("P2P stream inference request send failed")?;
    bytes_up = bytes_up.saturating_add(request_stats.bytes_sent);
    retry_count = retry_count.saturating_add(request_stats.retry_count);
    let connect_ms = duration_ms(started_at.elapsed());

    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(32);
    let model = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
    let response_task_id = task_id.clone();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let max_tokens_effective = request.max_tokens.unwrap_or(1024);
    let consumer_id = state.consumer_id;
    let timeout_secs = state.args.p2p_response_timeout_secs;
    let target_client_id_hex = hex::encode(target_client_id);

    tokio::spawn(async move {
        let result = receive_p2p_text_stream(
            &socket,
            peer_addr,
            connection_id,
            data_plane_secret,
            &task_id,
            timeout_secs,
            started_at,
            &tx,
            &model,
            created,
            &target_client_id_hex,
            max_tokens_effective,
        )
        .await;

        match result {
            Ok(mut result) => {
                result.bytes_up = result.bytes_up.saturating_add(bytes_up);
                result.retry_count = result.retry_count.saturating_add(retry_count);
                result.connect_ms = connect_ms;
                if let Err(e) = report_p2p_usage(
                    &mut control,
                    consumer_id,
                    target_client_id,
                    &request,
                    request_id,
                    &result,
                )
                .await
                {
                    warn!("failed to report P2P stream usage: {e:#}");
                }
                send_sse_data(&tx, "[DONE]".to_string()).await;
            }
            Err(e) => {
                warn!("P2P stream inference failed after response started: {e:#}");
                send_sse_json(
                    &tx,
                    json!({
                        "error": {
                            "message": e.to_string(),
                            "type": "api_error",
                            "code": 500
                        }
                    }),
                )
                .await;
                send_sse_data(&tx, "[DONE]".to_string()).await;
            }
        }
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|item| (item, rx))
    });
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("x-gpuf-p2p", "direct")
        .header("x-gpuf-client-id", hex::encode(target_client_id))
        .header("x-gpuf-p2p-task-id", response_task_id)
        .body(Body::from_stream(stream))?;
    response.headers_mut().insert(
        HeaderName::from_static("access-control-expose-headers"),
        HeaderValue::from_static("*"),
    );
    Ok(response)
}

async fn connect_and_login(state: &AppState, api_token: String) -> Result<Box<dyn ControlIo>> {
    let addr = format!("{}:{}", state.args.server_addr, state.args.control_port);
    let tcp = TcpStream::connect(&addr)
        .await
        .with_context(|| format!("failed to connect gpuf-s control at {addr}"))?;
    let mut control: Box<dyn ControlIo> = if state.args.control_tls {
        let certs = load_root_cert(&state.args.cert_chain_path)?;
        let mut roots = RootCertStore::empty();
        roots.add_parsable_certificates(certs);
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let name = state
            .args
            .control_tls_server_name
            .clone()
            .unwrap_or_else(|| state.args.server_addr.clone());
        let server_name =
            ServerName::try_from(name).map_err(|_| anyhow!("invalid control TLS server name"))?;
        Box::new(connector.connect(server_name, tcp).await?)
    } else {
        Box::new(tcp)
    };

    let login = Command::V2(CommandV2::P2PConsumerLogin {
        consumer_id: state.consumer_id,
        api_token: RedactedString::from(api_token),
    });
    write_command(&mut control, &login).await?;
    control.flush().await?;

    let mut buf = bytes::BytesMut::with_capacity(MAX_MESSAGE_SIZE);
    let cmd = timeout(
        Duration::from_secs(state.args.p2p_connect_timeout_secs),
        read_command(&mut control, &mut buf),
    )
    .await??;
    match cmd {
        Command::V2(CommandV2::P2PConsumerLoginResult {
            success: true,
            error: None,
            ..
        }) => Ok(control),
        Command::V2(CommandV2::P2PConsumerLoginResult {
            success: false,
            error,
            ..
        }) => Err(anyhow!(
            "gpuf-s rejected P2P consumer login: {}",
            error.unwrap_or_else(|| "unknown error".to_string())
        )),
        other => Err(anyhow!(
            "unexpected command after P2P consumer login: {other:?}"
        )),
    }
}

async fn wait_for_p2p_config(
    control: &mut Box<dyn ControlIo>,
    connection_id: [u8; 16],
    timeout_secs: u64,
) -> Result<([u8; 32], Vec<P2PCandidate>)> {
    let mut buf = bytes::BytesMut::with_capacity(MAX_MESSAGE_SIZE);
    timeout(Duration::from_secs(timeout_secs), async {
        let mut data_plane_secret: Option<[u8; 32]> = None;
        loop {
            let cmd = read_command(control, &mut buf).await?;
            match cmd {
                Command::V2(CommandV2::P2PConnectionConfig {
                    connection_id: cid,
                    data_plane_secret: secret,
                    ..
                }) if cid == connection_id => {
                    data_plane_secret = Some(secret.into_inner());
                }
                Command::V2(CommandV2::P2PCandidates {
                    connection_id: cid,
                    candidates,
                    ..
                }) if cid == connection_id => {
                    let secret = data_plane_secret.ok_or_else(|| {
                        anyhow!("P2PCandidates arrived before P2PConnectionConfig")
                    })?;
                    return Ok((secret, candidates));
                }
                other => {
                    debug!("ignoring control command while waiting P2P config: {other:?}");
                }
            }
        }
    })
    .await?
}

fn select_udp_peer(candidates: &[P2PCandidate]) -> Result<SocketAddr> {
    candidates
        .iter()
        .filter(|candidate| {
            candidate.transport == P2PTransport::Udp
                && matches!(
                    candidate.candidate_type,
                    P2PCandidateType::Host | P2PCandidateType::Srflx
                )
        })
        .max_by_key(|candidate| candidate.priority)
        .ok_or_else(|| anyhow!("no direct UDP P2P candidate received"))?
        .addr
        .parse()
        .context("invalid P2P candidate address")
}

async fn receive_p2p_text_result(
    socket: &UdpSocket,
    peer_addr: SocketAddr,
    connection_id: [u8; 16],
    data_plane_secret: [u8; 32],
    task_id: &str,
    timeout_secs: u64,
    started_at: Instant,
) -> Result<P2PTextResult> {
    let mut out = String::new();
    let mut analysis_tokens = 0;
    let mut final_tokens = 0;
    let mut bytes_down = 0u64;
    let mut chunk_count = 0u32;
    let mut ttft_ms = None;
    let mut buf = vec![0u8; 64 * 1024];
    let mut inflight: HashMap<u32, HashMap<u16, Vec<u8>>> = HashMap::new();

    loop {
        let (n, from) = timeout(
            Duration::from_secs(timeout_secs),
            socket.recv_from(&mut buf),
        )
        .await
        .context("P2P inference response timeout")??;
        if from != peer_addr {
            continue;
        }
        bytes_down = bytes_down.saturating_add(n as u64);
        let Some((flags, msg_id, frag_idx, frag_cnt, ts, tag)) = p2p_udp_parse_header(&buf[..n])
        else {
            continue;
        };
        if (flags & P2P_UDP_FLAG_ACK) != 0 {
            continue;
        }
        if n < P2P_UDP_HEADER_LEN {
            continue;
        }
        let payload = &buf[P2P_UDP_HEADER_LEN..n];
        p2p_udp_validate_fragment(
            &data_plane_secret,
            &connection_id,
            flags,
            msg_id,
            frag_idx,
            frag_cnt,
            ts,
            payload,
            &tag,
        )?;
        p2p_udp_send_ack(socket, from, connection_id, data_plane_secret, msg_id).await;

        let entry = inflight.entry(msg_id).or_default();
        entry.insert(frag_idx, payload.to_vec());
        let Some(full) = p2p_udp_try_reassemble(entry, frag_cnt) else {
            continue;
        };
        inflight.remove(&msg_id);
        let cmd = udp_decode_command(&full)?;
        match cmd {
            Command::V2(CommandV2::P2PInferenceChunk {
                connection_id: cid,
                task_id: tid,
                delta,
                error,
                analysis_tokens: chunk_analysis,
                final_tokens: chunk_final,
                ..
            }) if cid == connection_id && tid == task_id => {
                if let Some(error) = error {
                    return Err(anyhow!("P2P inference error: {error}"));
                }
                chunk_count = chunk_count.saturating_add(1);
                if ttft_ms.is_none() {
                    ttft_ms = Some(duration_ms(started_at.elapsed()));
                }
                analysis_tokens = analysis_tokens.max(chunk_analysis);
                final_tokens = final_tokens.max(chunk_final);
                out.push_str(&delta);
            }
            Command::V2(CommandV2::P2PInferenceDone {
                connection_id: cid,
                task_id: tid,
                prompt_tokens: done_prompt,
                completion_tokens: done_completion,
                total_tokens: done_total,
                analysis_tokens: done_analysis,
                final_tokens: done_final,
            }) if cid == connection_id && tid == task_id => {
                analysis_tokens = analysis_tokens.max(done_analysis);
                final_tokens = final_tokens.max(done_final);
                let completion_tokens = if done_completion == 0 {
                    estimate_token_count(&out)
                } else {
                    done_completion
                };
                let total_tokens = if done_total == 0 {
                    done_prompt.saturating_add(completion_tokens)
                } else {
                    done_total
                };
                let output_sha256 = Sha256::digest(out.as_bytes()).into();
                return Ok(P2PTextResult {
                    connection_id,
                    task_id: task_id.to_string(),
                    text: out,
                    prompt_tokens: done_prompt,
                    completion_tokens,
                    total_tokens,
                    analysis_tokens,
                    final_tokens,
                    bytes_up: 0,
                    bytes_down,
                    chunk_count,
                    retry_count: 0,
                    connect_ms: 0,
                    ttft_ms: ttft_ms.or_else(|| Some(duration_ms(started_at.elapsed()))),
                    total_ms: duration_ms(started_at.elapsed()),
                    output_sha256,
                });
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn receive_p2p_text_stream(
    socket: &UdpSocket,
    peer_addr: SocketAddr,
    connection_id: [u8; 16],
    data_plane_secret: [u8; 32],
    task_id: &str,
    timeout_secs: u64,
    started_at: Instant,
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
    model: &str,
    created: u64,
    target_client_id: &str,
    max_tokens_effective: u32,
) -> Result<P2PTextResult> {
    let mut out = String::new();
    let mut analysis_tokens = 0;
    let mut final_tokens = 0;
    let mut bytes_down = 0u64;
    let mut chunk_count = 0u32;
    let mut ttft_ms = None;
    let mut buf = vec![0u8; 64 * 1024];
    let mut inflight: HashMap<u32, HashMap<u16, Vec<u8>>> = HashMap::new();

    loop {
        let (n, from) = timeout(
            Duration::from_secs(timeout_secs),
            socket.recv_from(&mut buf),
        )
        .await
        .context("P2P stream response timeout")??;
        if from != peer_addr {
            continue;
        }
        bytes_down = bytes_down.saturating_add(n as u64);
        let Some((flags, msg_id, frag_idx, frag_cnt, ts, tag)) = p2p_udp_parse_header(&buf[..n])
        else {
            continue;
        };
        if (flags & P2P_UDP_FLAG_ACK) != 0 {
            continue;
        }
        if n < P2P_UDP_HEADER_LEN {
            continue;
        }
        let payload = &buf[P2P_UDP_HEADER_LEN..n];
        p2p_udp_validate_fragment(
            &data_plane_secret,
            &connection_id,
            flags,
            msg_id,
            frag_idx,
            frag_cnt,
            ts,
            payload,
            &tag,
        )?;
        p2p_udp_send_ack(socket, from, connection_id, data_plane_secret, msg_id).await;

        let entry = inflight.entry(msg_id).or_default();
        entry.insert(frag_idx, payload.to_vec());
        let Some(full) = p2p_udp_try_reassemble(entry, frag_cnt) else {
            continue;
        };
        inflight.remove(&msg_id);
        let cmd = udp_decode_command(&full)?;
        match cmd {
            Command::V2(CommandV2::P2PInferenceChunk {
                connection_id: cid,
                task_id: tid,
                delta,
                phase,
                error,
                analysis_tokens: chunk_analysis,
                final_tokens: chunk_final,
                ..
            }) if cid == connection_id && tid == task_id => {
                if let Some(error) = error {
                    return Err(anyhow!("P2P stream inference error: {error}"));
                }
                chunk_count = chunk_count.saturating_add(1);
                if ttft_ms.is_none() {
                    ttft_ms = Some(duration_ms(started_at.elapsed()));
                }
                analysis_tokens = analysis_tokens.max(chunk_analysis);
                final_tokens = final_tokens.max(chunk_final);
                if delta.is_empty() {
                    continue;
                }
                out.push_str(&delta);
                let delta_payload = match phase {
                    common::OutputPhase::Analysis => {
                        json!({"role": "assistant", "reasoning_content": delta})
                    }
                    _ => json!({"role": "assistant", "content": delta}),
                };
                send_sse_serialized(
                    tx,
                    &OpenAIChatStreamChunk {
                        id: task_id.to_string(),
                        object: "chat.completion.chunk",
                        created,
                        model: model.to_string(),
                        client_id: target_client_id.to_string(),
                        p2p: OpenAIP2PInfo {
                            enabled: true,
                            transport: "udp",
                            fallback: false,
                        },
                        choices: vec![OpenAIChatStreamChoice {
                            index: 0,
                            delta: delta_payload,
                            finish_reason: None,
                        }],
                        usage: None,
                    },
                )
                .await;
            }
            Command::V2(CommandV2::P2PInferenceDone {
                connection_id: cid,
                task_id: tid,
                prompt_tokens: done_prompt,
                completion_tokens: done_completion,
                total_tokens: done_total,
                analysis_tokens: done_analysis,
                final_tokens: done_final,
            }) if cid == connection_id && tid == task_id => {
                analysis_tokens = analysis_tokens.max(done_analysis);
                final_tokens = final_tokens.max(done_final);
                let completion_tokens = if done_completion == 0 {
                    estimate_token_count(&out)
                } else {
                    done_completion
                };
                let total_tokens = if done_total == 0 {
                    done_prompt.saturating_add(completion_tokens)
                } else {
                    done_total
                };
                let response_final_tokens = normalized_response_final_tokens(
                    completion_tokens,
                    analysis_tokens,
                    final_tokens,
                );
                let finish_reason = if completion_tokens >= max_tokens_effective {
                    "length"
                } else {
                    "stop"
                };
                send_sse_serialized(
                    tx,
                    &OpenAIChatStreamChunk {
                        id: task_id.to_string(),
                        object: "chat.completion.chunk",
                        created,
                        model: model.to_string(),
                        client_id: target_client_id.to_string(),
                        p2p: OpenAIP2PInfo {
                            enabled: true,
                            transport: "udp",
                            fallback: false,
                        },
                        choices: vec![OpenAIChatStreamChoice {
                            index: 0,
                            delta: json!({"role": "assistant"}),
                            finish_reason: Some(finish_reason),
                        }],
                        usage: Some(OpenAIUsage {
                            prompt_tokens: done_prompt,
                            completion_tokens,
                            total_tokens,
                            analysis_tokens,
                            final_tokens: response_final_tokens,
                        }),
                    },
                )
                .await;

                let output_sha256 = Sha256::digest(out.as_bytes()).into();
                return Ok(P2PTextResult {
                    connection_id,
                    task_id: task_id.to_string(),
                    text: out,
                    prompt_tokens: done_prompt,
                    completion_tokens,
                    total_tokens,
                    analysis_tokens,
                    final_tokens,
                    bytes_up: 0,
                    bytes_down,
                    chunk_count,
                    retry_count: 0,
                    connect_ms: 0,
                    ttft_ms: ttft_ms.or_else(|| Some(duration_ms(started_at.elapsed()))),
                    total_ms: duration_ms(started_at.elapsed()),
                    output_sha256,
                });
            }
            _ => {}
        }
    }
}

async fn send_sse_json(tx: &mpsc::Sender<Result<Bytes, Infallible>>, value: Value) {
    send_sse_data(tx, value.to_string()).await;
}

async fn send_sse_serialized<T: Serialize>(
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
    value: &T,
) {
    match serde_json::to_string(value) {
        Ok(data) => send_sse_data(tx, data).await,
        Err(e) => {
            send_sse_json(
                tx,
                json!({
                    "error": {
                        "message": format!("failed to serialize SSE chunk: {e}"),
                        "type": "api_error",
                        "code": 500
                    }
                }),
            )
            .await;
        }
    }
}

async fn send_sse_data(tx: &mpsc::Sender<Result<Bytes, Infallible>>, data: String) {
    let _ = tx.send(Ok(Bytes::from(format!("data: {data}\n\n")))).await;
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

async fn report_p2p_usage(
    control: &mut Box<dyn ControlIo>,
    consumer_id: [u8; 16],
    target_client_id: [u8; 16],
    request: &ChatCompletionRequest,
    request_id: Option<String>,
    result: &P2PTextResult,
) -> Result<()> {
    let report = Command::V2(CommandV2::P2PUsageReport {
        consumer_id,
        target_client_id,
        connection_id: result.connection_id,
        task_id: result.task_id.clone(),
        request_id,
        model: request.model.clone().unwrap_or_else(|| "gpuf".to_string()),
        endpoint: usage_endpoint_for_request(request).to_string(),
        transport: P2PUsageTransport::DirectUdp,
        stream: request.stream.unwrap_or(false),
        multimodal: request_is_multimodal(request),
        prompt_tokens: result.prompt_tokens,
        completion_tokens: result.completion_tokens,
        total_tokens: result.total_tokens,
        analysis_tokens: result.analysis_tokens,
        final_tokens: result.final_tokens,
        bytes_up: result.bytes_up,
        bytes_down: result.bytes_down,
        chunk_count: result.chunk_count,
        retry_count: result.retry_count,
        connect_ms: result.connect_ms,
        ttft_ms: result.ttft_ms,
        total_ms: result.total_ms,
        success: true,
        error: None,
        output_sha256: Some(result.output_sha256),
    });
    write_command(control, &report).await?;
    control.flush().await?;
    Ok(())
}

fn openai_chat_response(
    model: String,
    target_client_id: [u8; 16],
    result: P2PTextResult,
) -> Response<Body> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let final_tokens = normalized_response_final_tokens(
        result.completion_tokens,
        result.analysis_tokens,
        result.final_tokens,
    );
    let body = OpenAIChatResponse {
        id: result.task_id.clone(),
        object: "chat.completion",
        created: now,
        model,
        client_id: hex::encode(target_client_id),
        p2p: OpenAIP2PInfo {
            enabled: true,
            transport: "udp",
            fallback: false,
        },
        choices: vec![OpenAIChatChoice {
            index: 0,
            message: OpenAIChatMessage {
                role: "assistant",
                content: result.text,
            },
            finish_reason: "stop",
        }],
        usage: OpenAIUsage {
            prompt_tokens: result.prompt_tokens,
            completion_tokens: result.completion_tokens,
            total_tokens: result.total_tokens,
            analysis_tokens: result.analysis_tokens,
            final_tokens,
        },
    };
    let mut response = Json(body).into_response();
    response.headers_mut().insert(
        HeaderName::from_static("x-gpuf-p2p"),
        HeaderValue::from_static("direct"),
    );
    response
}

fn json_error(status: StatusCode, message: String) -> Response<Body> {
    (status, Json(json!({"error": {"message": message}}))).into_response()
}

fn estimate_token_count(text: &str) -> u32 {
    text.split_whitespace().count().max(1) as u32
}

fn normalized_response_final_tokens(
    completion_tokens: u32,
    analysis_tokens: u32,
    final_tokens: u32,
) -> u32 {
    if final_tokens > 0 {
        return final_tokens;
    }

    completion_tokens.saturating_sub(analysis_tokens.min(completion_tokens))
}

fn parse_client_id_hex(s: &str) -> Result<[u8; 16]> {
    let trimmed = s.trim().trim_start_matches("0x");
    if trimmed.len() != 32 {
        return Err(anyhow!(
            "client_id must be 32 hex characters, got {}",
            trimmed.len()
        ));
    }
    let bytes = hex::decode(trimmed).context("invalid client_id hex")?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("client_id must decode to 16 bytes"))
}

fn load_root_cert(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let f = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(f);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path);
    }
    Ok(certs)
}

fn udp_encode_command(command: &Command) -> Result<Vec<u8>> {
    let config = bincode_config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();
    let payload = bincode::encode_to_vec(command, config)?;
    let len = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn udp_decode_command(datagram: &[u8]) -> Result<Command> {
    if datagram.len() < 4 {
        return Err(anyhow!("udp datagram too short"));
    }
    let len = u32::from_be_bytes([datagram[0], datagram[1], datagram[2], datagram[3]]) as usize;
    if datagram.len() < 4 + len {
        return Err(anyhow!("udp datagram truncated"));
    }
    let config = bincode_config::standard()
        .with_fixed_int_encoding()
        .with_little_endian();
    let (cmd, _) = bincode::decode_from_slice(&datagram[4..4 + len], config)
        .map_err(|e| anyhow!("failed to deserialize command: {e}"))?;
    Ok(cmd)
}

fn p2p_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn p2p_hmac_sha256(secret: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac sha256 key");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn p2p_udp_tag(
    secret: &[u8; 32],
    connection_id: &[u8; 16],
    flags: u8,
    msg_id: u32,
    frag_idx: u16,
    frag_cnt: u16,
    timestamp: u64,
    payload: &[u8],
) -> [u8; 32] {
    let mut data = Vec::with_capacity(32 + 16 + 1 + 4 + 2 + 2 + 8 + payload.len());
    data.extend_from_slice(b"GPUF-P2P-UDP-V2");
    data.extend_from_slice(connection_id);
    data.push(flags);
    data.extend_from_slice(&msg_id.to_be_bytes());
    data.extend_from_slice(&frag_idx.to_be_bytes());
    data.extend_from_slice(&frag_cnt.to_be_bytes());
    data.extend_from_slice(&timestamp.to_be_bytes());
    data.extend_from_slice(payload);
    p2p_hmac_sha256(secret, &data)
}

fn p2p_udp_make_header(
    flags: u8,
    msg_id: u32,
    frag_idx: u16,
    frag_cnt: u16,
    timestamp: u64,
    tag: &[u8; 32],
) -> [u8; P2P_UDP_HEADER_LEN] {
    let mut h = [0u8; P2P_UDP_HEADER_LEN];
    h[0..4].copy_from_slice(&P2P_UDP_MAGIC);
    h[4] = P2P_UDP_VERSION;
    h[5] = flags;
    h[6..10].copy_from_slice(&msg_id.to_be_bytes());
    h[10..12].copy_from_slice(&frag_idx.to_be_bytes());
    h[12..14].copy_from_slice(&frag_cnt.to_be_bytes());
    h[14..22].copy_from_slice(&timestamp.to_be_bytes());
    h[22..54].copy_from_slice(tag);
    h
}

fn p2p_udp_parse_header(buf: &[u8]) -> Option<(u8, u32, u16, u16, u64, [u8; 32])> {
    if buf.len() < P2P_UDP_HEADER_LEN {
        return None;
    }
    if buf.get(0..4)? != P2P_UDP_MAGIC {
        return None;
    }
    if buf[4] != P2P_UDP_VERSION {
        return None;
    }
    let flags = buf[5];
    let msg_id = u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]]);
    let frag_idx = u16::from_be_bytes([buf[10], buf[11]]);
    let frag_cnt = u16::from_be_bytes([buf[12], buf[13]]);
    let timestamp = u64::from_be_bytes([
        buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20], buf[21],
    ]);
    let tag = buf[22..54].try_into().ok()?;
    Some((flags, msg_id, frag_idx, frag_cnt, timestamp, tag))
}

fn p2p_timestamp_is_fresh(timestamp: u64, now: u64) -> bool {
    timestamp <= now.saturating_add(30) && now.saturating_sub(timestamp) <= P2P_REPLAY_WINDOW_SECS
}

fn p2p_udp_validate_fragment(
    secret: &[u8; 32],
    connection_id: &[u8; 16],
    flags: u8,
    msg_id: u32,
    frag_idx: u16,
    frag_cnt: u16,
    timestamp: u64,
    payload: &[u8],
    tag: &[u8; 32],
) -> Result<()> {
    if (flags & P2P_UDP_FLAG_ACK) != 0 {
        if frag_idx != 0 || frag_cnt != 0 || !payload.is_empty() {
            return Err(anyhow!("invalid p2p udp ack metadata"));
        }
    } else {
        if frag_cnt == 0 || frag_cnt as usize > P2P_MAX_FRAGMENTS_PER_MESSAGE {
            return Err(anyhow!("invalid p2p udp fragment count"));
        }
        if frag_idx >= frag_cnt {
            return Err(anyhow!("invalid p2p udp fragment index"));
        }
    }
    if !p2p_timestamp_is_fresh(timestamp, p2p_now_secs()) {
        return Err(anyhow!("stale p2p udp fragment"));
    }
    let expected = p2p_udp_tag(
        secret,
        connection_id,
        flags,
        msg_id,
        frag_idx,
        frag_cnt,
        timestamp,
        payload,
    );
    if expected.as_slice() != tag.as_slice() {
        return Err(anyhow!("p2p udp fragment authentication failed"));
    }
    Ok(())
}

async fn p2p_udp_send_ack(
    socket: &UdpSocket,
    to: SocketAddr,
    connection_id: [u8; 16],
    secret: [u8; 32],
    msg_id: u32,
) {
    let timestamp = p2p_now_secs();
    let tag = p2p_udp_tag(
        &secret,
        &connection_id,
        P2P_UDP_FLAG_ACK,
        msg_id,
        0,
        0,
        timestamp,
        &[],
    );
    let hdr = p2p_udp_make_header(P2P_UDP_FLAG_ACK, msg_id, 0, 0, timestamp, &tag);
    let _ = socket.send_to(&hdr, to).await;
}

async fn p2p_udp_send_reliable(
    socket: &UdpSocket,
    to: SocketAddr,
    connection_id: [u8; 16],
    secret: [u8; 32],
    msg_id: u32,
    payload: &[u8],
) -> Result<P2PUdpSendStats> {
    let max_payload = P2P_UDP_MTU_PAYLOAD.saturating_sub(P2P_UDP_HEADER_LEN);
    if max_payload == 0 {
        return Err(anyhow!("p2p udp mtu too small"));
    }
    let frag_cnt = ((payload.len() + max_payload - 1) / max_payload).max(1);
    if frag_cnt > P2P_MAX_FRAGMENTS_PER_MESSAGE {
        return Err(anyhow!("p2p udp too many fragments"));
    }

    let mut stats = P2PUdpSendStats::default();
    for frag_idx in 0..frag_cnt {
        let start = frag_idx * max_payload;
        let end = ((frag_idx + 1) * max_payload).min(payload.len());
        let frag_payload = &payload[start..end];
        let timestamp = p2p_now_secs();
        let tag = p2p_udp_tag(
            &secret,
            &connection_id,
            0,
            msg_id,
            frag_idx as u16,
            frag_cnt as u16,
            timestamp,
            frag_payload,
        );
        let hdr = p2p_udp_make_header(0, msg_id, frag_idx as u16, frag_cnt as u16, timestamp, &tag);
        let mut pkt = Vec::with_capacity(P2P_UDP_HEADER_LEN + frag_payload.len());
        pkt.extend_from_slice(&hdr);
        pkt.extend_from_slice(frag_payload);

        let mut tries = 0u32;
        loop {
            tries += 1;
            socket.send_to(&pkt, to).await?;
            stats.bytes_sent = stats.bytes_sent.saturating_add(pkt.len() as u64);
            if tries > 1 {
                stats.retry_count = stats.retry_count.saturating_add(1);
            }

            let mut ack_buf = [0u8; P2P_UDP_HEADER_LEN];
            let ack_res = timeout(Duration::from_millis(400), socket.recv_from(&mut ack_buf)).await;
            if let Ok(Ok((n, from))) = ack_res {
                if from != to {
                    continue;
                }
                if let Some((flags, ack_id, ack_frag_idx, ack_frag_cnt, ts, ack_tag)) =
                    p2p_udp_parse_header(&ack_buf[..n])
                {
                    let valid_ack = (flags & P2P_UDP_FLAG_ACK) != 0
                        && ack_id == msg_id
                        && ack_frag_idx == 0
                        && ack_frag_cnt == 0
                        && p2p_udp_validate_fragment(
                            &secret,
                            &connection_id,
                            flags,
                            ack_id,
                            ack_frag_idx,
                            ack_frag_cnt,
                            ts,
                            &[],
                            &ack_tag,
                        )
                        .is_ok();
                    if valid_ack {
                        break;
                    }
                }
            }
            if tries >= 10 {
                return Err(anyhow!("p2p udp send timeout msg_id={msg_id}"));
            }
        }
    }
    Ok(stats)
}

fn p2p_udp_try_reassemble(parts: &mut HashMap<u16, Vec<u8>>, frag_cnt: u16) -> Option<Vec<u8>> {
    if frag_cnt == 0 {
        return None;
    }
    for i in 0..frag_cnt {
        if !parts.contains_key(&i) {
            return None;
        }
    }
    let mut out = Vec::new();
    for i in 0..frag_cnt {
        if let Some(part) = parts.remove(&i) {
            out.extend_from_slice(&part);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_parser_accepts_hex_and_0x_prefix() {
        let plain = parse_client_id_hex("00112233445566778899aabbccddeeff").unwrap();
        let prefixed = parse_client_id_hex("0x00112233445566778899aabbccddeeff").unwrap();
        assert_eq!(plain, prefixed);
    }

    #[test]
    fn chat_payload_accepts_multimodal_chat_task() {
        let request = ChatCompletionRequest {
            model: Some("PaddleOCR-VL-1.6-GGUF".to_string()),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: ChatMessageContent::Parts(vec![ChatContentPart {
                    r#type: "image_url".to_string(),
                    text: None,
                    image_url: None,
                    image: None,
                }]),
            }],
            max_tokens: None,
            temperature: None,
            top_k: None,
            top_p: None,
            repeat_penalty: None,
            repeat_last_n: None,
            min_keep: None,
            stream: None,
        };
        let payload = chat_request_to_payload(&request).unwrap();
        assert!(request_is_multimodal(&request));
        let command = p2p_inference_command([1u8; 16], "task", &request, payload);
        match command {
            Command::V2(CommandV2::ChatInferenceTask {
                messages, model, ..
            }) => {
                assert_eq!(model, "PaddleOCR-VL-1.6-GGUF");
                assert_eq!(messages.len(), 1);
                assert!(messages[0].content.is_multimodal());
            }
            other => panic!("unexpected command: {other:?}"),
        }
        assert_eq!(usage_endpoint_for_request(&request), "ocr.image");
    }

    #[test]
    fn udp_command_payload_round_trips() {
        let connection_id = [3u8; 16];
        let command = Command::V2(CommandV2::P2PInferenceDone {
            connection_id,
            task_id: "task".to_string(),
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            analysis_tokens: 0,
            final_tokens: 2,
        });
        let encoded = udp_encode_command(&command).unwrap();
        let decoded = udp_decode_command(&encoded).unwrap();
        match decoded {
            Command::V2(CommandV2::P2PInferenceDone { total_tokens, .. }) => {
                assert_eq!(total_tokens, 3);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn normalized_response_final_tokens_falls_back_to_completion_tokens() {
        assert_eq!(normalized_response_final_tokens(60, 0, 0), 60);
        assert_eq!(normalized_response_final_tokens(60, 10, 0), 50);
        assert_eq!(normalized_response_final_tokens(60, 10, 42), 42);
    }
}
