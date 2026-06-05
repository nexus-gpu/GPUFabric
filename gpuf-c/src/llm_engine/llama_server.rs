// HTTP API Server for LlamaEngine (OpenAI compatible)
use super::llama_engine::{LlamaEngine, SamplingParams};
use crate::util::security_metrics;
use anyhow::Result;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{sse, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};
use tracing::{error, info};

#[derive(Clone)]
pub struct ApiServerState {
    pub engine: Arc<RwLock<LlamaEngine>>,
    pub security: Arc<ServerSecurityConfig>,
    generation_semaphore: Arc<Semaphore>,
    sse_semaphore: Arc<Semaphore>,
}

impl ApiServerState {
    fn new(engine: Arc<RwLock<LlamaEngine>>, security: ServerSecurityConfig) -> Self {
        let max_generations = security.limits.max_concurrent_generations.max(1);
        let max_sse = security.limits.max_sse_connections.max(1);
        Self {
            engine,
            security: Arc::new(security),
            generation_semaphore: Arc::new(Semaphore::new(max_generations)),
            sse_semaphore: Arc::new(Semaphore::new(max_sse)),
        }
    }

    pub(crate) fn try_generation_permit(&self) -> Result<OwnedSemaphorePermit, AppError> {
        self.generation_semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                security_metrics::record_rate_limit_rejection();
                AppError::too_many_requests("generation concurrency limit exceeded")
            })
    }

    pub(crate) fn try_sse_permit(&self) -> Result<OwnedSemaphorePermit, AppError> {
        self.sse_semaphore.clone().try_acquire_owned().map_err(|_| {
            security_metrics::record_rate_limit_rejection();
            AppError::too_many_requests("SSE connection limit exceeded")
        })
    }
}

#[derive(Debug, Clone)]
pub struct ServerSecurityConfig {
    pub api_key: Option<String>,
    pub limits: SecurityLimits,
    pub content_safety: ContentSafetyConfig,
}

impl ServerSecurityConfig {
    pub fn from_env() -> Self {
        let api_key = std::env::var("GPUF_API_KEY")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        Self {
            api_key,
            limits: SecurityLimits::from_env(),
            content_safety: ContentSafetyConfig::from_env(),
        }
    }
}

impl Default for ServerSecurityConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug, Clone)]
pub struct SecurityLimits {
    pub max_prompt_bytes: usize,
    pub max_max_tokens: usize,
    pub max_concurrent_generations: usize,
    pub max_sse_connections: usize,
    pub request_body_limit_bytes: usize,
}

impl SecurityLimits {
    pub fn from_env() -> Self {
        Self {
            max_prompt_bytes: read_usize_env("GPUF_MAX_PROMPT_BYTES", 128 * 1024),
            max_max_tokens: read_usize_env("GPUF_MAX_MAX_TOKENS", 4096),
            max_concurrent_generations: read_usize_env("GPUF_MAX_CONCURRENT_GENERATIONS", 2),
            max_sse_connections: read_usize_env("GPUF_MAX_SSE_CONNECTIONS", 8),
            request_body_limit_bytes: read_usize_env("GPUF_REQUEST_BODY_LIMIT_BYTES", 1024 * 1024),
        }
    }
}

impl Default for SecurityLimits {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContentSafetyConfig {
    pub enabled: bool,
    pub blocked_keywords: Vec<String>,
    pub block_special_tokens: bool,
}

impl ContentSafetyConfig {
    pub fn from_env() -> Self {
        let blocked_keywords = read_csv_env("GPUF_BLOCKED_KEYWORDS");
        let block_special_tokens = read_bool_env("GPUF_BLOCK_SPECIAL_TOKENS");
        let enabled = read_bool_env("GPUF_CONTENT_SAFETY")
            || read_bool_env("GPUF_CONTENT_SAFETY_ENABLED")
            || block_special_tokens
            || !blocked_keywords.is_empty();

        Self {
            enabled,
            blocked_keywords,
            block_special_tokens,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled && (!self.blocked_keywords.is_empty() || self.block_special_tokens)
    }
}

fn read_usize_env(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn read_bool_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn read_csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim())
                .filter(|item| !item.is_empty())
                .map(|item| item.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// OpenAI compatible chat completion request
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_k: Option<i32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,
    #[serde(default)]
    pub repeat_last_n: Option<i32>,
    #[serde(default)]
    pub seed: Option<u32>,
    #[serde(default)]
    pub min_keep: Option<usize>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// OpenAI compatible chat completion response
#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub finish_reason: String,
}

/// Streaming chunk for chat completion
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoiceChunk>,
}

#[derive(Debug, Serialize)]
pub struct ChatChoiceChunk {
    pub index: usize,
    pub delta: ChatMessageDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatMessageDelta {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Text completion request
#[derive(Debug, Deserialize)]
pub struct CompletionRequest {
    pub model: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_k: Option<i32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,
    #[serde(default)]
    pub repeat_last_n: Option<i32>,
    #[serde(default)]
    pub seed: Option<u32>,
    #[serde(default)]
    pub min_keep: Option<usize>,
}

/// Text completion response
#[derive(Debug, Serialize)]
pub struct CompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<CompletionChoice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct CompletionChoice {
    pub index: usize,
    pub text: String,
    pub finish_reason: String,
}

/// Model list response
#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<ModelData>,
}

#[derive(Debug, Serialize)]
pub struct ModelData {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub model_loaded: bool,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub message: String,
    pub r#type: String,
}

/// Create HTTP API server
pub fn create_router(engine: Arc<RwLock<LlamaEngine>>) -> Router {
    create_router_with_security(engine, ServerSecurityConfig::from_env())
}

pub fn create_router_with_security(
    engine: Arc<RwLock<LlamaEngine>>,
    security: ServerSecurityConfig,
) -> Router {
    let body_limit = security.limits.request_body_limit_bytes;
    let state = ApiServerState::new(engine, security);

    let protected_routes = Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/security/metrics", get(security_metrics_handler))
        .route(
            "/v1/messages",
            post(super::anthropic_server::messages_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    Router::new()
        .route("/health", get(health_check))
        .merge(protected_routes)
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(state)
}

/// Health check
async fn health_check(State(state): State<ApiServerState>) -> Json<HealthResponse> {
    let engine = state.engine.read().await;
    let model_loaded = engine.is_ready().await;

    Json(HealthResponse {
        status: "ok".to_string(),
        model_loaded,
    })
}

/// List models
async fn list_models(
    State(state): State<ApiServerState>,
) -> Result<Json<ModelsResponse>, AppError> {
    let engine = state.engine.read().await;
    let models = engine.list_local_models().await?;

    let data = models
        .into_iter()
        .map(|id| ModelData {
            id,
            object: "model".to_string(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            owned_by: "llama.cpp".to_string(),
        })
        .collect();

    Ok(Json(ModelsResponse {
        object: "list".to_string(),
        data,
    }))
}

async fn security_metrics_handler() -> Json<security_metrics::SecurityMetricsSnapshot> {
    Json(security_metrics::snapshot())
}

/// Chat completion
async fn chat_completions(
    State(state): State<ApiServerState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    info!(
        "Chat completion request: {} messages, stream: {}",
        req.messages.len(),
        req.stream
    );

    // Build prompt
    let prompt = build_chat_prompt(&req.messages);
    validate_prompt_and_tokens(&state.security.limits, &prompt, req.max_tokens)?;
    validate_content_safety(&state.security.content_safety, &prompt, "prompt")?;

    let max_tokens = req.max_tokens.unwrap_or(100);
    let mut sampling = SamplingParams::default();
    if let Some(v) = req.temperature {
        sampling.temperature = v;
    }
    if let Some(v) = req.top_k {
        sampling.top_k = v;
    }
    if let Some(v) = req.top_p {
        sampling.top_p = v;
    }
    if let Some(v) = req.repeat_penalty {
        sampling.repeat_penalty = v;
    }
    if let Some(v) = req.repeat_last_n {
        sampling.repeat_last_n = v;
    }
    if let Some(v) = req.seed {
        sampling.seed = v;
    }
    if let Some(v) = req.min_keep {
        sampling.min_keep = v;
    }

    let model_name = req.model.unwrap_or_else(|| "llama.cpp".to_string());
    let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if req.stream {
        let generation_permit = state.try_generation_permit()?;
        let sse_permit = state.try_sse_permit()?;
        let engine = state.engine.read().await;

        // True streaming: use stream_with_cached_model_sampling
        // When SSE disconnects, the channel send fails and inference stops
        let token_stream = engine
            .stream_with_cached_model_sampling(&prompt, max_tokens, &sampling)
            .await?;

        // Convert token stream to SSE stream with proper OpenAI format.
        let id = id.clone();
        let created = created;
        let model_name = model_name.clone();
        let content_safety = state.security.content_safety.clone();
        let output_filter_state = Arc::new(Mutex::new((false, String::new())));
        let token_events = token_stream.filter_map(move |result| {
            let id = id.clone();
            let model_name = model_name.clone();
            let content_safety = content_safety.clone();
            let output_filter_state = Arc::clone(&output_filter_state);

            async move {
                if output_filter_state
                    .lock()
                    .map(|state| state.0)
                    .unwrap_or(true)
                {
                    return None;
                }

                let event = match result {
                    Ok(token) => {
                        {
                            let mut state = output_filter_state
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            state.1.push_str(&token);
                            if state.1.len() > 65_536 {
                                let trim_to = state.1.len() - 65_536;
                                state.1.drain(..trim_to);
                            }

                            if let Err(err) =
                                validate_content_safety(&content_safety, &state.1, "output")
                            {
                                state.0 = true;
                                return Some(Ok::<_, std::convert::Infallible>(
                                    sse::Event::default()
                                        .event("error")
                                        .data(err.public_message),
                                ));
                            }
                        }

                        let chunk = ChatCompletionChunk {
                            id,
                            object: "chat.completion.chunk".to_string(),
                            created,
                            model: model_name,
                            choices: vec![ChatChoiceChunk {
                                index: 0,
                                delta: ChatMessageDelta {
                                    role: "assistant".to_string(),
                                    content: token,
                                },
                                finish_reason: None,
                            }],
                        };
                        Ok::<_, std::convert::Infallible>(
                            sse::Event::default().json_data(chunk).unwrap_or_else(|_| {
                                sse::Event::default()
                                    .event("error")
                                    .data("json serialization failed")
                            }),
                        )
                    }
                    Err(e) => {
                        error!("OpenAI stream token error: {}", e);
                        Ok::<_, std::convert::Infallible>(
                            sse::Event::default().event("error").data("stream error"),
                        )
                    }
                };

                Some(event)
            }
        });
        let done = stream::once(async {
            Ok::<_, std::convert::Infallible>(sse::Event::default().data("[DONE]"))
        });
        let permits = Arc::new((generation_permit, sse_permit));
        let stream = token_events.chain(done).map(move |event| {
            let _keep_permits_alive = &permits;
            event
        });

        Ok(sse::Sse::new(stream).into_response())
    } else {
        let _generation_permit = state.try_generation_permit()?;
        let engine = state.engine.read().await;

        // Non-streaming response
        let (response_text, prompt_tokens, completion_tokens) = engine
            .generate_with_cached_model_sampling(&prompt, max_tokens, &sampling)
            .await?;
        validate_content_safety(&state.security.content_safety, &response_text, "output")?;

        let response = ChatCompletionResponse {
            id,
            object: "chat.completion".to_string(),
            created,
            model: model_name,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: response_text,
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        };

        Ok(Json(response).into_response())
    }
}

/// Text completion
async fn completions(
    State(state): State<ApiServerState>,
    Json(req): Json<CompletionRequest>,
) -> Result<Json<CompletionResponse>, AppError> {
    info!(
        "Completion request received: prompt_bytes={}",
        req.prompt.len()
    );

    validate_prompt_and_tokens(&state.security.limits, &req.prompt, req.max_tokens)?;
    validate_content_safety(&state.security.content_safety, &req.prompt, "prompt")?;
    let _generation_permit = state.try_generation_permit()?;
    let engine = state.engine.read().await;
    let max_tokens = req.max_tokens.unwrap_or(100);
    let mut sampling = SamplingParams::default();
    if let Some(v) = req.temperature {
        sampling.temperature = v;
    }
    if let Some(v) = req.top_k {
        sampling.top_k = v;
    }
    if let Some(v) = req.top_p {
        sampling.top_p = v;
    }
    if let Some(v) = req.repeat_penalty {
        sampling.repeat_penalty = v;
    }
    if let Some(v) = req.repeat_last_n {
        sampling.repeat_last_n = v;
    }
    if let Some(v) = req.seed {
        sampling.seed = v;
    }
    if let Some(v) = req.min_keep {
        sampling.min_keep = v;
    }

    let (response_text, prompt_tokens, completion_tokens) = engine
        .generate_with_cached_model_sampling(&req.prompt, max_tokens, &sampling)
        .await?;
    validate_content_safety(&state.security.content_safety, &response_text, "output")?;

    let response = CompletionResponse {
        id: format!("cmpl-{}", uuid::Uuid::new_v4()),
        object: "text_completion".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        model: req.model.unwrap_or_else(|| "llama.cpp".to_string()),
        choices: vec![CompletionChoice {
            index: 0,
            text: response_text,
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        },
    };

    Ok(Json(response))
}

/// Build chat prompt using various popular formats
/// You can set CHAT_TEMPLATE env var to: chatml, llama3, alpaca, or simple (default)
pub(crate) fn build_chat_prompt(messages: &[ChatMessage]) -> String {
    let template = std::env::var("CHAT_TEMPLATE").unwrap_or_else(|_| "simple".to_string());

    match template.to_lowercase().as_str() {
        "chatml" => build_chatml_prompt(messages),
        "llama3" => build_llama3_prompt(messages),
        "alpaca" => build_alpaca_prompt(messages),
        _ => build_simple_prompt(messages),
    }
}

/// ChatML format (Qwen, GPT-4, etc.)
pub(crate) fn build_chatml_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();
    for msg in messages {
        prompt.push_str(&format!(
            "<|im_start|>{}\n{}<|im_end|>\n",
            msg.role, msg.content
        ));
    }
    prompt.push_str("<|im_start|>assistant\n");
    prompt
}

/// Llama 3 format
pub(crate) fn build_llama3_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::from("<|begin_of_text|>");
    for msg in messages {
        prompt.push_str(&format!(
            "<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>",
            msg.role, msg.content
        ));
    }
    prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
    prompt
}

/// Alpaca/Vicuna format (broad compatibility)
pub(crate) fn build_alpaca_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();
    for msg in messages {
        match msg.role.as_str() {
            "system" => prompt.push_str(&format!("### Instruction:\n{}\n\n", msg.content)),
            "user" => prompt.push_str(&format!("### Input:\n{}\n\n", msg.content)),
            "assistant" => prompt.push_str(&format!("### Response:\n{}\n\n", msg.content)),
            _ => prompt.push_str(&format!("### {}:\n{}\n\n", msg.role, msg.content)),
        }
    }
    prompt.push_str("### Response:\n");
    prompt
}

/// Simple format (most universal, works with almost any model)
pub(crate) fn build_simple_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();
    for msg in messages {
        prompt.push_str(&format!(
            "{}: {}\n\n",
            match msg.role.as_str() {
                "user" => "Human",
                "assistant" => "Assistant",
                "system" => "System",
                _ => &msg.role,
            },
            msg.content
        ));
    }
    prompt.push_str("Assistant:");
    prompt
}

pub(crate) fn validate_prompt_and_tokens(
    limits: &SecurityLimits,
    prompt: &str,
    requested_max_tokens: Option<usize>,
) -> Result<(), AppError> {
    if prompt.len() > limits.max_prompt_bytes {
        security_metrics::record_prompt_rejection();
        return Err(AppError::payload_too_large(format!(
            "prompt too large: {} bytes exceeds limit {}",
            prompt.len(),
            limits.max_prompt_bytes
        )));
    }

    if let Some(max_tokens) = requested_max_tokens {
        if max_tokens > limits.max_max_tokens {
            security_metrics::record_max_token_rejection();
            return Err(AppError::bad_request(format!(
                "max_tokens too large: {} exceeds limit {}",
                max_tokens, limits.max_max_tokens
            )));
        }
    }

    Ok(())
}

async fn require_api_key(
    State(state): State<ApiServerState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    if let Some(expected) = state.security.api_key.as_deref() {
        if !is_authorized(req.headers(), expected) {
            security_metrics::record_auth_failure();
            return Err(AppError::unauthorized());
        }
    }

    Ok(next.run(req).await)
}

fn is_authorized(headers: &HeaderMap, expected: &str) -> bool {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if let Some(token) = bearer {
        return constant_time_eq(token.as_bytes(), expected.as_bytes());
    }

    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(|token| constant_time_eq(token.as_bytes(), expected.as_bytes()))
        .unwrap_or(false)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim().trim_matches('[').trim_matches(']');
    if normalized.eq_ignore_ascii_case("localhost") {
        return true;
    }

    normalized
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn validate_content_safety(
    content_safety: &ContentSafetyConfig,
    text: &str,
    direction: &str,
) -> Result<(), AppError> {
    if !content_safety.is_enabled() {
        return Ok(());
    }

    let lower = text.to_lowercase();
    for keyword in &content_safety.blocked_keywords {
        let needle = keyword.trim().to_lowercase();
        if !needle.is_empty() && lower.contains(&needle) {
            security_metrics::record_content_filter_rejection();
            return Err(AppError::content_filter(format!(
                "{} contains blocked keyword '{}'",
                direction, keyword
            )));
        }
    }

    if content_safety.block_special_tokens {
        const SPECIAL_PATTERNS: [&str; 10] = [
            "<|",
            "<s>",
            "</s>",
            "[INST]",
            "[/INST]",
            "<|im_start|>",
            "<|im_end|>",
            "<|begin_of_text|>",
            "<|start_header_id|>",
            "<|end_header_id|>",
        ];
        if SPECIAL_PATTERNS
            .iter()
            .any(|pattern| text.contains(pattern))
        {
            security_metrics::record_content_filter_rejection();
            return Err(AppError::content_filter(format!(
                "{} contains blocked special token marker",
                direction
            )));
        }
    }

    Ok(())
}

/// Error handling
#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    public_message: String,
    error_type: &'static str,
    source: Option<anyhow::Error>,
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            public_message: message.into(),
            error_type: "invalid_request_error",
            source: None,
        }
    }

    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            public_message: "missing or invalid API key".to_string(),
            error_type: "authentication_error",
            source: None,
        }
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            public_message: message.into(),
            error_type: "invalid_request_error",
            source: None,
        }
    }

    pub fn content_filter(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            public_message: message.into(),
            error_type: "content_filter_error",
            source: None,
        }
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            public_message: message.into(),
            error_type: "rate_limit_error",
            source: None,
        }
    }

    fn internal(err: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            public_message: "internal server error".to_string(),
            error_type: "internal_error",
            source: Some(err),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Some(source) = &self.source {
            error!("API error: {}", source);
        } else {
            error!("API error: {}", self.public_message);
        }

        let error_response = ErrorResponse {
            error: ErrorDetail {
                message: self.public_message,
                r#type: self.error_type.to_string(),
            },
        };

        (self.status, Json(error_response)).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self::internal(err.into())
    }
}

/// Start HTTP server
pub async fn start_server(engine: Arc<RwLock<LlamaEngine>>, host: &str, port: u16) -> Result<()> {
    start_server_with_security(engine, host, port, ServerSecurityConfig::from_env()).await
}

pub async fn start_server_with_security(
    engine: Arc<RwLock<LlamaEngine>>,
    host: &str,
    port: u16,
    security: ServerSecurityConfig,
) -> Result<()> {
    if security.api_key.is_none() && !is_loopback_host(host) {
        return Err(anyhow::anyhow!(
            "API key is required when binding standalone API server to non-loopback host {}. Set --api-key or GPUF_API_KEY, or bind to 127.0.0.1.",
            host
        ));
    }

    let has_api_key = security.api_key.is_some();
    let body_limit = security.limits.request_body_limit_bytes;
    let app = create_router_with_security(engine, security);
    let addr = format!("{}:{}", host, port);

    info!(
        "Starting Llama API server on {} (auth={}, request_body_limit_bytes={})",
        addr,
        if has_api_key {
            "enabled"
        } else {
            "loopback-only"
        },
        body_limit
    );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn bearer_and_x_api_key_authorize() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-secret"),
        );
        assert!(is_authorized(&headers, "test-secret"));

        headers.clear();
        headers.insert("x-api-key", HeaderValue::from_static("test-secret"));
        assert!(is_authorized(&headers, "test-secret"));
        assert!(!is_authorized(&headers, "other-secret"));
    }

    #[test]
    fn prompt_and_token_limits_reject_before_generation() {
        let before = security_metrics::snapshot();
        let limits = SecurityLimits {
            max_prompt_bytes: 4,
            max_max_tokens: 8,
            max_concurrent_generations: 1,
            max_sse_connections: 1,
            request_body_limit_bytes: 32,
        };
        assert!(validate_prompt_and_tokens(&limits, "abcd", Some(8)).is_ok());
        assert!(validate_prompt_and_tokens(&limits, "abcde", Some(8)).is_err());
        assert!(validate_prompt_and_tokens(&limits, "abcd", Some(9)).is_err());
        let after = security_metrics::snapshot();
        assert!(after.prompt_rejections >= before.prompt_rejections + 1);
        assert!(after.max_token_rejections >= before.max_token_rejections + 1);
    }

    #[test]
    fn content_safety_is_opt_in_and_records_rejections() {
        let before = security_metrics::snapshot();
        let disabled = ContentSafetyConfig::default();
        assert!(validate_content_safety(&disabled, "leak secret", "prompt").is_ok());

        let enabled = ContentSafetyConfig {
            enabled: true,
            blocked_keywords: vec!["secret".to_string()],
            block_special_tokens: true,
        };
        assert!(validate_content_safety(&enabled, "leak secret", "prompt").is_err());
        assert!(validate_content_safety(&enabled, "<|im_start|>", "output").is_err());
        let after = security_metrics::snapshot();
        assert!(after.content_filter_rejections >= before.content_filter_rejections + 2);
    }

    #[test]
    fn public_bind_requires_api_key() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(!is_loopback_host("0.0.0.0"));
    }
}
