// HTTP API Server for LlamaEngine (OpenAI compatible)
use super::llama_engine::{LlamaEngine, SamplingParams};
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

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

/// Application state
pub struct AppState {
    pub engine: Arc<RwLock<LlamaEngine>>,
}

/// Create HTTP API server
pub fn create_router(engine: Arc<RwLock<LlamaEngine>>) -> Router {
    let state = Arc::new(AppState { engine });

    Router::new()
        .route("/health", get(health_check))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .with_state(state)
}

/// Health check
async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let engine = state.engine.read().await;
    let model_loaded = engine.is_ready().await;

    Json(HealthResponse {
        status: "ok".to_string(),
        model_loaded,
    })
}

/// List models
async fn list_models(State(state): State<Arc<AppState>>) -> Result<Json<ModelsResponse>, AppError> {
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

/// Chat completion
async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, AppError> {
    info!("Chat completion request: {} messages", req.messages.len());

    // Build prompt
    let prompt = build_chat_prompt(&req.messages);

    // Generate text
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
        .generate_with_cached_model_sampling(&prompt, max_tokens, &sampling)
        .await?;

    let response = ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        model: req.model.unwrap_or_else(|| "llama.cpp".to_string()),
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

    Ok(Json(response))
}

/// Text completion
async fn completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompletionRequest>,
) -> Result<Json<CompletionResponse>, AppError> {
    info!("Completion request: {}", req.prompt);

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
fn build_chat_prompt(messages: &[ChatMessage]) -> String {
    let template = std::env::var("CHAT_TEMPLATE").unwrap_or_else(|_| "simple".to_string());

    match template.to_lowercase().as_str() {
        "chatml" => build_chatml_prompt(messages),
        "llama3" => build_llama3_prompt(messages),
        "alpaca" => build_alpaca_prompt(messages),
        _ => build_simple_prompt(messages),
    }
}

/// ChatML format (Qwen, GPT-4, etc.)
fn build_chatml_prompt(messages: &[ChatMessage]) -> String {
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
fn build_llama3_prompt(messages: &[ChatMessage]) -> String {
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
fn build_alpaca_prompt(messages: &[ChatMessage]) -> String {
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
fn build_simple_prompt(messages: &[ChatMessage]) -> String {
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

/// Error handling
struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("API error: {}", self.0);

        let error_response = ErrorResponse {
            error: ErrorDetail {
                message: self.0.to_string(),
                r#type: "internal_error".to_string(),
            },
        };

        (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

/// Start HTTP server
pub async fn start_server(engine: Arc<RwLock<LlamaEngine>>, host: &str, port: u16) -> Result<()> {
    let app = create_router(engine);
    let addr = format!("{}:{}", host, port);

    info!("Starting Llama API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
