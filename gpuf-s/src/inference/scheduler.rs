use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::handle::ActiveClients;
use crate::util::protoc::ClientId;
use common::{
    ChatMessageContent, ChatMessageV2, Command, CommandV1, CommandV2, EngineType, OsType,
    OutputPhase, COMMAND_V1_EMBEDDING_TASKS_VERSION,
};

// Type aliases for easier function signatures
// Note: Can't create type alias for enum variants in Rust

// OpenAI Compatible Request/Response Types
#[derive(Debug, Deserialize)]
pub struct CompletionRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub repeat_last_n: Option<i32>,
    pub min_keep: Option<u32>,
    #[allow(dead_code)] // Part of OpenAI API spec, will be used later
    pub model: Option<String>,
    #[allow(dead_code)] // Streaming support to be implemented later
    pub stream: Option<bool>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cache_policy: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub repeat_last_n: Option<i32>,
    pub min_keep: Option<u32>,
    pub stream: Option<bool>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cache_policy: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Single(String),
    Batch(Vec<String>),
}

impl EmbeddingInput {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::Single(input) => vec![input],
            Self::Batch(inputs) => inputs,
        }
    }
}

fn default_embedding_normalize() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: EmbeddingInput,
    #[serde(default)]
    pub encoding_format: Option<String>,
    #[serde(default = "default_embedding_normalize")]
    pub normalize: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SophnetEmbeddingRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub input_texts: Vec<String>,
    #[serde(default)]
    pub input_images: Option<Vec<String>>,
    pub dimensions: u32,
    pub easyllm_id: String,
    #[serde(default)]
    pub normalized: Option<bool>,
    #[serde(default)]
    pub encoding_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatMessageContent,
}

#[derive(Debug, Serialize)]
pub struct CompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<ClientId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_status: Option<SessionCacheStatus>,
    pub choices: Vec<CompletionChoice>,
    pub usage: CompletionUsage,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<ClientId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_status: Option<SessionCacheStatus>,
    pub p2p: P2PResponseInfo,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: CompletionUsage,
}

#[derive(Debug, Serialize, Clone)]
pub struct P2PResponseInfo {
    pub enabled: bool,
    pub transport: String,
    pub fallback: bool,
}

impl P2PResponseInfo {
    pub fn gateway() -> Self {
        Self {
            enabled: false,
            transport: "gateway".to_string(),
            fallback: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CompletionChoice {
    pub text: String,
    pub index: i32,
    pub logprobs: Option<serde_json::Value>,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChoice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub analysis_tokens: Option<u32>,
    pub final_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

#[derive(Debug, Clone)]
pub struct EmbeddingTaskResult {
    pub embeddings: Vec<Vec<f32>>,
    pub prompt_tokens: u32,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingResponse {
    pub object: String,
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingData {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: usize,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Serialize)]
pub struct SophnetEmbeddingResponse {
    pub id: String,
    pub object: String,
    pub usage: SophnetEmbeddingUsage,
    pub data: Vec<EmbeddingData>,
}

#[derive(Debug, Serialize)]
pub struct SophnetEmbeddingUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: Option<u32>,
    pub total_tokens: u32,
    pub prompt_tokens_details: Option<serde_json::Value>,
    pub completion_tokens_details: Option<serde_json::Value>,
}

// Task result tracking
type PendingTask = oneshot::Sender<Result<CompletionResponse>>;
type PendingEmbeddingTask = oneshot::Sender<Result<EmbeddingTaskResult>>;

#[derive(Debug)]
pub enum StreamEvent {
    Delta(String, OutputPhase),
    Finish(Option<CompletionUsage>),
    Done,
    Error(String),
}

const SESSION_ROUTE_TTL: Duration = Duration::from_secs(60 * 60);
const DEFAULT_SESSION_ROUTE_MAX_ENTRIES: usize = 1024;

fn positive_env_usize(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn positive_env_duration_secs(name: &str, default_value: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_secs)
        .unwrap_or(default_value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    Auto,
    Bypass,
    Reset,
}

impl CachePolicy {
    pub fn parse(raw: Option<&str>) -> Result<Self> {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(Self::Auto),
            Some(value) if value.eq_ignore_ascii_case("auto") => Ok(Self::Auto),
            Some(value) if value.eq_ignore_ascii_case("bypass") => Ok(Self::Bypass),
            Some(value) if value.eq_ignore_ascii_case("reset") => Ok(Self::Reset),
            Some(value) => Err(anyhow!(
                "invalid cache_policy '{value}', expected auto, bypass, or reset"
            )),
        }
    }

    fn records_route(self) -> bool {
        !matches!(self, Self::Bypass)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bypass => "bypass",
            Self::Reset => "reset",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionCacheStatus {
    Cold,
    Hit,
    Bypass,
    Reset,
    Evicted,
}

impl SessionCacheStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Hit => "hit",
            Self::Bypass => "bypass",
            Self::Reset => "reset",
            Self::Evicted => "evicted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRouteOutcome {
    pub session_id: Option<String>,
    pub client_id: ClientId,
    pub cache_status: Option<SessionCacheStatus>,
}

impl SessionRouteOutcome {
    pub fn new(
        session_id: Option<String>,
        client_id: ClientId,
        cache_status: Option<SessionCacheStatus>,
    ) -> Self {
        Self {
            session_id,
            client_id,
            cache_status,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct SessionRouteMetricsSnapshot {
    pub routes_current: usize,
    pub routes_max: usize,
    pub route_ttl_secs: u64,
    pub sticky_route_hit_total: u64,
    pub sticky_route_miss_total: u64,
    pub sticky_route_bypass_total: u64,
    pub sticky_route_reset_total: u64,
    pub sticky_route_bind_total: u64,
    pub sticky_route_eviction_total: u64,
    pub sticky_route_stale_total: u64,
    pub sticky_route_denied_total: u64,
    pub session_owner_mismatch_total: u64,
}

#[derive(Debug, Clone, Default)]
struct SessionRouteMetrics {
    sticky_route_hit_total: u64,
    sticky_route_miss_total: u64,
    sticky_route_bypass_total: u64,
    sticky_route_reset_total: u64,
    sticky_route_bind_total: u64,
    sticky_route_eviction_total: u64,
    sticky_route_stale_total: u64,
    sticky_route_denied_total: u64,
    session_owner_mismatch_total: u64,
}

impl SessionRouteMetrics {
    fn snapshot(
        &self,
        routes_current: usize,
        routes_max: usize,
        route_ttl: Duration,
    ) -> SessionRouteMetricsSnapshot {
        SessionRouteMetricsSnapshot {
            routes_current,
            routes_max,
            route_ttl_secs: route_ttl.as_secs(),
            sticky_route_hit_total: self.sticky_route_hit_total,
            sticky_route_miss_total: self.sticky_route_miss_total,
            sticky_route_bypass_total: self.sticky_route_bypass_total,
            sticky_route_reset_total: self.sticky_route_reset_total,
            sticky_route_bind_total: self.sticky_route_bind_total,
            sticky_route_eviction_total: self.sticky_route_eviction_total,
            sticky_route_stale_total: self.sticky_route_stale_total,
            sticky_route_denied_total: self.sticky_route_denied_total,
            session_owner_mismatch_total: self.session_owner_mismatch_total,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRouting {
    pub session_id: String,
    pub owner_scope: String,
    pub cache_policy: CachePolicy,
    pub model_id: Option<String>,
    pub explicit_target: bool,
}

impl SessionRouting {
    pub fn new(
        session_id: String,
        owner_scope: String,
        cache_policy: CachePolicy,
        model_id: Option<String>,
        explicit_target: bool,
    ) -> Self {
        Self {
            session_id,
            owner_scope,
            cache_policy,
            model_id,
            explicit_target,
        }
    }
}

#[derive(Debug, Clone)]
struct SessionRoute {
    owner_scope: String,
    client_id: ClientId,
    model_id: Option<String>,
    #[allow(dead_code)] // Retained for observability/Redis persistence in the P4 follow-up.
    created_at: Instant,
    last_used: Instant,
    ttl: Duration,
}

struct SessionRouteTable {
    routes: HashMap<String, SessionRoute>,
    max_routes: usize,
    route_ttl: Duration,
    metrics: SessionRouteMetrics,
}

impl Default for SessionRouteTable {
    fn default() -> Self {
        Self::new(DEFAULT_SESSION_ROUTE_MAX_ENTRIES, SESSION_ROUTE_TTL)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SessionRouteDecision {
    Use {
        client_id: ClientId,
        cache_status: SessionCacheStatus,
    },
    SelectCold(SessionCacheStatus),
}

impl SessionRouteTable {
    fn new(max_routes: usize, route_ttl: Duration) -> Self {
        Self {
            routes: HashMap::new(),
            max_routes,
            route_ttl,
            metrics: SessionRouteMetrics::default(),
        }
    }

    fn snapshot(&self) -> SessionRouteMetricsSnapshot {
        self.metrics
            .snapshot(self.routes.len(), self.max_routes, self.route_ttl)
    }

    fn prune_expired(&mut self, now: Instant) {
        let expired: Vec<String> = self
            .routes
            .iter()
            .filter(|(_, route)| now.duration_since(route.last_used) > route.ttl)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        if !expired.is_empty() {
            self.metrics.sticky_route_eviction_total += expired.len() as u64;
        }
        for session_id in expired {
            self.routes.remove(&session_id);
        }
    }

    fn evict_lru(&mut self) -> Option<String> {
        let victim = self
            .routes
            .iter()
            .min_by_key(|(_, route)| route.last_used)
            .map(|(session_id, _)| session_id.clone());
        if let Some(session_id) = &victim {
            self.routes.remove(session_id);
            self.metrics.sticky_route_eviction_total += 1;
        }
        victim
    }

    fn resolve(
        &mut self,
        routing: &SessionRouting,
        allowed_client_ids: Option<&[ClientId]>,
        now: Instant,
    ) -> Result<SessionRouteDecision> {
        if matches!(routing.cache_policy, CachePolicy::Bypass) {
            self.metrics.sticky_route_bypass_total += 1;
            return Ok(SessionRouteDecision::SelectCold(SessionCacheStatus::Bypass));
        }

        if matches!(routing.cache_policy, CachePolicy::Reset) {
            self.metrics.sticky_route_reset_total += 1;
            self.remove_owned_route(routing)?;
            return Ok(SessionRouteDecision::SelectCold(SessionCacheStatus::Reset));
        }

        let Some(route) = self.routes.get_mut(&routing.session_id) else {
            self.metrics.sticky_route_miss_total += 1;
            return Ok(SessionRouteDecision::SelectCold(SessionCacheStatus::Cold));
        };

        if route.owner_scope != routing.owner_scope {
            self.metrics.session_owner_mismatch_total += 1;
            return Err(anyhow!("session owner mismatch"));
        }

        if now.duration_since(route.last_used) > route.ttl {
            self.routes.remove(&routing.session_id);
            self.metrics.sticky_route_stale_total += 1;
            return Ok(SessionRouteDecision::SelectCold(
                SessionCacheStatus::Evicted,
            ));
        }

        if model_conflicts(route.model_id.as_deref(), routing.model_id.as_deref()) {
            self.routes.remove(&routing.session_id);
            self.metrics.sticky_route_stale_total += 1;
            return Ok(SessionRouteDecision::SelectCold(
                SessionCacheStatus::Evicted,
            ));
        }

        if routing.explicit_target {
            if let Some(allowed) = allowed_client_ids {
                if !allowed.iter().any(|id| id == &route.client_id) {
                    self.routes.remove(&routing.session_id);
                    self.metrics.sticky_route_stale_total += 1;
                    return Ok(SessionRouteDecision::SelectCold(
                        SessionCacheStatus::Evicted,
                    ));
                }
            }
        } else if let Some(allowed) = allowed_client_ids {
            if !allowed.iter().any(|id| id == &route.client_id) {
                self.metrics.sticky_route_denied_total += 1;
                return Err(anyhow!(
                    "sticky session route is no longer allowed for this token"
                ));
            }
        }

        route.last_used = now;
        self.metrics.sticky_route_hit_total += 1;
        Ok(SessionRouteDecision::Use {
            client_id: route.client_id,
            cache_status: SessionCacheStatus::Hit,
        })
    }

    fn bind(&mut self, routing: &SessionRouting, client_id: ClientId, now: Instant) {
        if !routing.cache_policy.records_route() {
            return;
        }

        self.prune_expired(now);

        if !self.routes.contains_key(&routing.session_id) && self.routes.len() >= self.max_routes {
            self.evict_lru();
        }

        self.routes.insert(
            routing.session_id.clone(),
            SessionRoute {
                owner_scope: routing.owner_scope.clone(),
                client_id,
                model_id: routing.model_id.clone(),
                created_at: now,
                last_used: now,
                ttl: self.route_ttl,
            },
        );
        self.metrics.sticky_route_bind_total += 1;
    }

    fn remove_owned_route(&mut self, routing: &SessionRouting) -> Result<()> {
        if let Some(route) = self.routes.get(&routing.session_id) {
            if route.owner_scope != routing.owner_scope {
                return Err(anyhow!("session owner mismatch"));
            }
        }
        self.routes.remove(&routing.session_id);
        Ok(())
    }

    fn remove_if_client_matches(&mut self, session_id: &str, client_id: ClientId) {
        if self
            .routes
            .get(session_id)
            .map(|route| route.client_id == client_id)
            .unwrap_or(false)
        {
            self.routes.remove(session_id);
            self.metrics.sticky_route_eviction_total += 1;
        }
    }
}

fn model_conflicts(route_model: Option<&str>, request_model: Option<&str>) -> bool {
    matches!((route_model, request_model), (Some(a), Some(b)) if a != b)
}

fn session_command_fields(routing: Option<&SessionRouting>) -> (Option<String>, Option<String>) {
    routing
        .map(|routing| {
            (
                Some(worker_scoped_session_id(
                    routing.owner_scope.as_str(),
                    routing.session_id.as_str(),
                )),
                Some(routing.cache_policy.as_str().to_string()),
            )
        })
        .unwrap_or((None, None))
}

fn worker_scoped_session_id(owner_scope: &str, session_id: &str) -> String {
    let mut hasher = Sha256::new();
    for part in ["worker-session-v1", owner_scope, session_id] {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

pub fn validate_session_id(raw: &str) -> Result<String> {
    let session_id = raw.trim();
    let len = session_id.len();
    if !(16..=128).contains(&len) {
        return Err(anyhow!(
            "session_id must be between 16 and 128 ASCII characters"
        ));
    }

    if !session_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(anyhow!(
            "session_id may only contain ASCII letters, digits, '-', '_', or '.'"
        ));
    }

    Ok(session_id.to_string())
}

// Inference Scheduler
pub struct InferenceScheduler {
    pending_tasks: Arc<Mutex<HashMap<String, PendingTask>>>,
    pending_embedding_tasks: Arc<Mutex<HashMap<String, PendingEmbeddingTask>>>,
    partial_results: Arc<Mutex<HashMap<String, String>>>,
    pending_streams: Arc<Mutex<HashMap<String, mpsc::Sender<StreamEvent>>>>,
    stream_usages: Arc<Mutex<HashMap<String, CompletionUsage>>>,
    session_routes: Arc<Mutex<SessionRouteTable>>,
    active_clients: ActiveClients,
}

impl InferenceScheduler {
    pub fn new(active_clients: ActiveClients) -> Self {
        let max_routes = positive_env_usize(
            "GPUF_SESSION_ROUTE_MAX_ENTRIES",
            DEFAULT_SESSION_ROUTE_MAX_ENTRIES,
        );
        let route_ttl =
            positive_env_duration_secs("GPUF_SESSION_ROUTE_TTL_SECS", SESSION_ROUTE_TTL);

        Self {
            pending_tasks: Arc::new(Mutex::new(HashMap::new())),
            pending_embedding_tasks: Arc::new(Mutex::new(HashMap::new())),
            partial_results: Arc::new(Mutex::new(HashMap::new())),
            pending_streams: Arc::new(Mutex::new(HashMap::new())),
            stream_usages: Arc::new(Mutex::new(HashMap::new())),
            session_routes: Arc::new(Mutex::new(SessionRouteTable::new(max_routes, route_ttl))),
            active_clients,
        }
    }

    pub async fn session_route_metrics(&self) -> SessionRouteMetricsSnapshot {
        let routes = self.session_routes.lock().await;
        routes.snapshot()
    }

    async fn client_is_usable(
        &self,
        client_id: &ClientId,
        allowed_client_ids: Option<&[ClientId]>,
        model_name: Option<&str>,
        require_model_compat: bool,
    ) -> bool {
        if let Some(allowed) = allowed_client_ids {
            if !allowed.iter().any(|id| id == client_id) {
                return false;
            }
        }

        let clients = self.active_clients.lock().await;
        let Some(client_info) = clients.get(client_id) else {
            return false;
        };

        if !client_info.authed {
            return false;
        }

        if require_model_compat {
            let Some(model_name) = model_name else {
                return true;
            };
            let Some(models) = &client_info.models else {
                return false;
            };
            if !models.iter().any(|m| m.id == model_name) {
                return false;
            }
        }

        true
    }

    async fn select_device_for_request(
        &self,
        allowed_client_ids: Option<&[ClientId]>,
        routing: Option<&SessionRouting>,
        model_name: Option<&str>,
        require_model_compat: bool,
    ) -> Result<SessionRouteOutcome> {
        if let Some(routing) = routing {
            if matches!(routing.cache_policy, CachePolicy::Bypass) {
                let selected = self
                    .select_fresh_device(model_name, allowed_client_ids, require_model_compat)
                    .await?;
                return Ok(SessionRouteOutcome::new(
                    Some(routing.session_id.clone()),
                    selected,
                    Some(SessionCacheStatus::Bypass),
                ));
            }

            let now = Instant::now();
            let sticky_device = {
                let mut routes = self.session_routes.lock().await;
                routes.resolve(routing, allowed_client_ids, now)?
            };

            let cold_status = match sticky_device {
                SessionRouteDecision::Use {
                    client_id,
                    cache_status,
                } => {
                    if self
                        .client_is_usable(
                            &client_id,
                            allowed_client_ids,
                            model_name,
                            require_model_compat,
                        )
                        .await
                    {
                        return Ok(SessionRouteOutcome::new(
                            Some(routing.session_id.clone()),
                            client_id,
                            Some(cache_status),
                        ));
                    }

                    let mut routes = self.session_routes.lock().await;
                    routes.remove_if_client_matches(&routing.session_id, client_id);
                    SessionCacheStatus::Evicted
                }
                SessionRouteDecision::SelectCold(status) => status,
            };

            let selected = self
                .select_fresh_device(model_name, allowed_client_ids, require_model_compat)
                .await?;
            if routing.cache_policy.records_route() {
                let mut routes = self.session_routes.lock().await;
                routes.bind(routing, selected, Instant::now());
            }
            return Ok(SessionRouteOutcome::new(
                Some(routing.session_id.clone()),
                selected,
                Some(cold_status),
            ));
        }

        self.select_fresh_device(model_name, allowed_client_ids, require_model_compat)
            .await
            .map(|client_id| SessionRouteOutcome::new(None, client_id, None))
    }

    async fn select_fresh_device(
        &self,
        model_name: Option<&str>,
        allowed_client_ids: Option<&[ClientId]>,
        require_model_compat: bool,
    ) -> Result<ClientId> {
        if !require_model_compat {
            return self.select_best_device(allowed_client_ids).await;
        }

        let Some(model_name) = model_name else {
            return self.select_best_device(allowed_client_ids).await;
        };

        match self
            .select_best_device_for_model(model_name, allowed_client_ids)
            .await
        {
            Ok(device_id) => Ok(device_id),
            Err(e) => {
                warn!(
                    "No model-compatible device found for model '{}': {}. Falling back to generic device selection.",
                    model_name, e
                );
                self.select_best_device(allowed_client_ids).await
            }
        }
    }

    pub async fn execute_inference_stream(
        &self,
        request: CompletionRequest,
        routing: Option<SessionRouting>,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<(String, SessionRouteOutcome, mpsc::Receiver<StreamEvent>)> {
        let task_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel::<StreamEvent>(128);

        {
            let mut streams = self.pending_streams.lock().await;
            streams.insert(task_id.clone(), tx);
        }

        let model_name = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
        let route_outcome = self
            .select_device_for_request(
                allowed_client_ids,
                routing.as_ref(),
                Some(model_name.as_str()),
                false,
            )
            .await?;
        let (session_id, cache_policy) = session_command_fields(routing.as_ref());
        if let Err(e) = self
            .send_task_to_device(
                &route_outcome.client_id,
                task_id.clone(),
                session_id,
                cache_policy,
                request.prompt,
                request.max_tokens.unwrap_or(4090),
                request.temperature.unwrap_or(0.7),
                request.top_k.unwrap_or(40),
                request.top_p.unwrap_or(0.9),
                request.repeat_penalty.unwrap_or(1.1),
                request.repeat_last_n.unwrap_or(64),
                request.min_keep.unwrap_or(1),
            )
            .await
        {
            let mut streams = self.pending_streams.lock().await;
            streams.remove(&task_id);
            return Err(e);
        }

        Ok((task_id, route_outcome, rx))
    }

    async fn select_best_device_for_model(
        &self,
        model_name: &str,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<ClientId> {
        let clients = self.active_clients.lock().await;

        let mut best_device: Option<(ClientId, u16)> = None;

        debug!("online Clients: {}", clients.len());
        for (client_id, client_info) in clients.iter() {
            if let Some(allowed) = allowed_client_ids {
                if !allowed.iter().any(|id| id == client_id) {
                    debug!("Client {} is not allowed", client_id.log_label());
                    continue;
                }
            }
            debug!(
                "Client {} is authed {} model {}",
                client_id.log_label(),
                client_info.authed,
                model_name
            );
            if !client_info.authed {
                continue;
            }
            let Some(models) = &client_info.models else {
                continue;
            };
            if !models.iter().any(|m| m.id == model_name) {
                continue;
            }

            let Some(system_info) = &client_info.system_info else {
                continue;
            };
            let total_load: u16 = (system_info.cpu_usage + system_info.memory_usage) as u16;

            match best_device {
                None => best_device = Some((*client_id, total_load)),
                Some((_best_id, best_load)) if total_load < best_load => {
                    best_device = Some((*client_id, total_load))
                }
                _ => {}
            }
        }

        best_device
            .map(|(id, _)| id)
            .ok_or_else(|| anyhow!("No compatible client found for model '{model_name}'"))
    }

    fn client_supports_embedding_tasks(client_info: &crate::handle::ClientInfo) -> bool {
        if client_info.version < COMMAND_V1_EMBEDDING_TASKS_VERSION {
            return false;
        }

        if client_info.os_type == OsType::ANDROID || client_info.os_type == OsType::IOS {
            return false;
        }

        if client_info
            .devices_info
            .iter()
            .any(|device| device.os_type == OsType::ANDROID || device.os_type == OsType::IOS)
        {
            return false;
        }

        client_info
            .devices_info
            .iter()
            .any(|device| device.engine_type == EngineType::Llama)
    }

    async fn select_best_embedding_device_for_model(
        &self,
        model_name: &str,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<ClientId> {
        let clients = self.active_clients.lock().await;

        let mut best_device: Option<(ClientId, u16)> = None;

        debug!("online Clients for embedding: {}", clients.len());
        for (client_id, client_info) in clients.iter() {
            if let Some(allowed) = allowed_client_ids {
                if !allowed.iter().any(|id| id == client_id) {
                    debug!("Client {} is not allowed", client_id.log_label());
                    continue;
                }
            }

            if !client_info.authed {
                continue;
            }
            if !Self::client_supports_embedding_tasks(client_info) {
                debug!(
                    "Skipping client {} for embedding because os_type={:?}",
                    client_id.log_label(),
                    client_info.os_type
                );
                continue;
            }

            let Some(models) = &client_info.models else {
                continue;
            };
            if !models.iter().any(|m| m.id == model_name) {
                continue;
            }

            let Some(system_info) = &client_info.system_info else {
                continue;
            };
            let total_load: u16 = (system_info.cpu_usage + system_info.memory_usage) as u16;

            match best_device {
                None => best_device = Some((*client_id, total_load)),
                Some((_best_id, best_load)) if total_load < best_load => {
                    best_device = Some((*client_id, total_load))
                }
                _ => {}
            }
        }

        best_device.map(|(id, _)| id).ok_or_else(|| {
            anyhow!("No non-mobile embedding-capable client found for model '{model_name}'")
        })
    }

    pub async fn execute_chat_inference_stream(
        &self,
        model: String,
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
        routing: Option<SessionRouting>,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<(String, SessionRouteOutcome, mpsc::Receiver<StreamEvent>)> {
        let task_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel::<StreamEvent>(128);

        {
            let mut streams = self.pending_streams.lock().await;
            streams.insert(task_id.clone(), tx);
        }

        let route_outcome = self
            .select_device_for_request(
                allowed_client_ids,
                routing.as_ref(),
                Some(model.as_str()),
                true,
            )
            .await?;
        let (session_id, cache_policy) = session_command_fields(routing.as_ref());
        debug!(
            "Selected device {} for model {}",
            route_outcome.client_id.log_label(),
            model
        );
        if let Err(e) = self
            .send_chat_task_to_device(
                &route_outcome.client_id,
                task_id.clone(),
                session_id,
                cache_policy,
                model,
                messages,
                max_tokens,
                temperature,
                top_k,
                top_p,
                repeat_penalty,
                repeat_last_n,
                min_keep,
            )
            .await
        {
            let mut streams = self.pending_streams.lock().await;
            streams.remove(&task_id);
            return Err(e);
        }

        Ok((task_id, route_outcome, rx))
    }

    pub async fn execute_embedding(
        &self,
        request: EmbeddingRequest,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<(EmbeddingResponse, SessionRouteOutcome)> {
        if let Some(format) = request.encoding_format.as_deref() {
            if !format.eq_ignore_ascii_case("float") {
                return Err(anyhow!(
                    "unsupported encoding_format '{}', only 'float' is supported",
                    format
                ));
            }
        }

        let input = request.input.into_vec();
        if input.is_empty() {
            return Err(anyhow!("embedding input must not be empty"));
        }
        if input.iter().any(|item| item.trim().is_empty()) {
            return Err(anyhow!("embedding input items must not be empty"));
        }
        let input_count = input.len();

        let task_id = Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        {
            let mut tasks = self.pending_embedding_tasks.lock().await;
            tasks.insert(task_id.clone(), sender);
        }

        let client_id = match self
            .select_best_embedding_device_for_model(&request.model, allowed_client_ids)
            .await
        {
            Ok(client_id) => client_id,
            Err(e) => {
                let mut tasks = self.pending_embedding_tasks.lock().await;
                tasks.remove(&task_id);
                return Err(e);
            }
        };
        let route_outcome = SessionRouteOutcome::new(None, client_id, None);

        if let Err(e) = self
            .send_embedding_task_to_device(
                &route_outcome.client_id,
                task_id.clone(),
                request.model.clone(),
                input,
                request.normalize,
            )
            .await
        {
            let mut tasks = self.pending_embedding_tasks.lock().await;
            tasks.remove(&task_id);
            return Err(e);
        }

        let timeout_secs: u64 = std::env::var("GPUF_EMBEDDING_TIMEOUT_SECS")
            .ok()
            .or_else(|| std::env::var("GPUF_INFERENCE_TIMEOUT_SECS").ok())
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(120);

        match tokio::time::timeout(Duration::from_secs(timeout_secs), receiver).await {
            Ok(Ok(Ok(result))) => {
                if result.embeddings.len() != input_count {
                    return Err(anyhow!(
                        "embedding result count mismatch: expected {}, got {}",
                        input_count,
                        result.embeddings.len()
                    ));
                }

                let data = result
                    .embeddings
                    .into_iter()
                    .enumerate()
                    .map(|(index, embedding)| EmbeddingData {
                        object: "embedding".to_string(),
                        embedding,
                        index,
                    })
                    .collect();
                let usage = EmbeddingUsage {
                    prompt_tokens: result.prompt_tokens,
                    total_tokens: result.prompt_tokens,
                };
                Ok((
                    EmbeddingResponse {
                        object: "list".to_string(),
                        data,
                        model: request.model,
                        usage,
                    },
                    route_outcome,
                ))
            }
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err(anyhow!("Embedding response channel closed")),
            Err(_) => {
                let mut tasks = self.pending_embedding_tasks.lock().await;
                tasks.remove(&task_id);
                Err(anyhow!(
                    "Embedding task timed out after {} seconds",
                    timeout_secs
                ))
            }
        }
    }

    pub async fn cancel_inference(&self, task_id: &str, device_id: &ClientId) -> Result<()> {
        debug!(
            "Cancelling inference for task {} on device {}",
            task_id,
            device_id.log_label()
        );
        {
            let mut streams = self.pending_streams.lock().await;
            streams.remove(task_id);
        }

        use common::write_command;

        let mut clients = self.active_clients.lock().await;
        let client_info = clients
            .get_mut(device_id)
            .ok_or_else(|| anyhow!("Device not found or not connected"))?;

        if !client_info.authed {
            return Err(anyhow!("Device not authenticated"));
        }

        let mut writer = client_info.writer.lock().await;

        let cancel = CommandV1::CancelInference {
            task_id: task_id.to_string(),
        };
        let command = Command::V1(cancel);
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn send_chat_task_to_device(
        &self,
        device_id: &ClientId,
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
    ) -> Result<()> {
        use common::write_command;

        let mut clients = self.active_clients.lock().await;
        let client_info = clients
            .get_mut(device_id)
            .ok_or_else(|| anyhow!("Device not found or not connected"))?;

        if !client_info.authed {
            error!("Device {} not authenticated", device_id.log_label());
            return Err(anyhow!("Device not authenticated"));
        }

        let mut writer = client_info
            .writer
            .try_lock()
            .map_err(|_| anyhow!("Device is busy, please try again"))?;

        let message_count = messages.len();
        let command = if messages
            .iter()
            .all(|message| matches!(message.content, ChatMessageContent::Text(_)))
        {
            Command::V1(CommandV1::ChatInferenceTask {
                task_id: task_id.clone(),
                session_id,
                cache_policy,
                model,
                messages: messages
                    .into_iter()
                    .map(|message| common::ChatMessage {
                        role: message.role,
                        content: match message.content {
                            ChatMessageContent::Text(text) => text,
                            ChatMessageContent::Parts(_) => unreachable!("checked above"),
                        },
                    })
                    .collect(),
                max_tokens,
                temperature,
                top_k,
                top_p,
                repeat_penalty,
                repeat_last_n,
                min_keep,
            })
        } else {
            Command::V2(CommandV2::ChatInferenceTask {
                task_id: task_id.clone(),
                session_id,
                cache_policy,
                model,
                messages: messages
                    .into_iter()
                    .map(|message| ChatMessageV2 {
                        role: message.role,
                        content: message.content,
                    })
                    .collect(),
                max_tokens,
                temperature,
                top_k,
                top_p,
                repeat_penalty,
                repeat_last_n,
                min_keep,
            })
        };
        info!(
            "sent chat inference task {} to device {} (messages={}, max_tokens={})",
            task_id,
            device_id.log_label(),
            message_count,
            max_tokens
        );
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn send_embedding_task_to_device(
        &self,
        device_id: &ClientId,
        task_id: String,
        model: String,
        input: Vec<String>,
        normalize: bool,
    ) -> Result<()> {
        use common::write_command;

        let mut clients = self.active_clients.lock().await;
        let client_info = clients
            .get_mut(device_id)
            .ok_or_else(|| anyhow!("Device not found or not connected"))?;

        if !client_info.authed {
            error!("Device {} not authenticated", device_id.log_label());
            return Err(anyhow!("Device not authenticated"));
        }

        let mut writer = client_info
            .writer
            .try_lock()
            .map_err(|_| anyhow!("Device is busy, please try again"))?;

        let input_count = input.len();
        let command = Command::V1(CommandV1::EmbeddingTask {
            task_id: task_id.clone(),
            model,
            input,
            normalize,
        });
        info!(
            "sent embedding task {} to device {} (inputs={})",
            task_id,
            device_id.log_label(),
            input_count
        );
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }

    pub async fn handle_inference_result_chunk(
        &self,
        task_id: String,
        _seq: u32,
        delta: String,
        phase: OutputPhase,
        done: bool,
        error: Option<String>,
        prompt_tokens: u32,
        completion_tokens: u32,
        analysis_tokens: u32,
        final_tokens: u32,
    ) {
        let stream_sender = {
            let streams = self.pending_streams.lock().await;
            streams.get(&task_id).cloned()
        };

        if let Some(sender) = stream_sender {
            if let Some(err) = error {
                let _ = sender.send(StreamEvent::Error(err)).await;
                let _ = sender.send(StreamEvent::Done).await;
                let mut streams = self.pending_streams.lock().await;
                streams.remove(&task_id);
                let mut usages = self.stream_usages.lock().await;
                usages.remove(&task_id);
                return;
            }

            if !delta.is_empty() {
                let _ = sender.send(StreamEvent::Delta(delta, phase)).await;
            }

            if done {
                let usage = CompletionUsage {
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens.saturating_add(completion_tokens),
                    analysis_tokens: Some(analysis_tokens),
                    final_tokens: Some(final_tokens),
                };
                {
                    let mut usages = self.stream_usages.lock().await;
                    usages.insert(task_id.clone(), usage.clone());
                }

                let usage_for_finish = {
                    let usages = self.stream_usages.lock().await;
                    usages.get(&task_id).cloned()
                };

                let _ = sender.send(StreamEvent::Finish(usage_for_finish)).await;
                let _ = sender.send(StreamEvent::Done).await;
                let mut streams = self.pending_streams.lock().await;
                streams.remove(&task_id);
                let mut usages = self.stream_usages.lock().await;
                usages.remove(&task_id);
            }
            return;
        }

        if let Some(err) = error {
            self.handle_inference_result(task_id, false, None, Some(err), 0, 0, 0)
                .await;
            return;
        }

        {
            let mut partial = self.partial_results.lock().await;
            let entry = partial.entry(task_id.clone()).or_insert_with(String::new);
            entry.push_str(&delta);
        }

        if done {
            let result = {
                let mut partial = self.partial_results.lock().await;
                partial.remove(&task_id).unwrap_or_default()
            };
            self.handle_inference_result(
                task_id,
                true,
                Some(result),
                None,
                0,
                prompt_tokens,
                completion_tokens,
            )
            .await;
        }
    }

    pub async fn handle_embedding_result(
        &self,
        task_id: String,
        success: bool,
        embeddings: Vec<Vec<f32>>,
        error: Option<String>,
        prompt_tokens: u32,
    ) {
        let sender = {
            let mut tasks = self.pending_embedding_tasks.lock().await;
            tasks.remove(&task_id)
        };

        let Some(sender) = sender else {
            debug!(
                "Dropping embedding result for task {} because it is no longer pending",
                task_id
            );
            return;
        };

        let response = if success {
            Ok(EmbeddingTaskResult {
                embeddings,
                prompt_tokens,
            })
        } else {
            Err(anyhow!("Embedding failed: {}", error.unwrap_or_default()))
        };

        if sender.send(response).is_err() {
            warn!("Failed to send embedding result for task {}", task_id);
        }
    }

    /// Handle inference result from device
    pub async fn handle_inference_result(
        &self,
        task_id: String,
        success: bool,
        result: Option<String>,
        error: Option<String>,
        _execution_time_ms: u64,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) {
        info!(
            "Handling inference result for task {} (success: {})",
            task_id, success
        );

        let mut tasks = self.pending_tasks.lock().await;
        let pending_count_before = tasks.len();
        info!("Current pending tasks count: {}", pending_count_before);

        // Find the sender for this taskretain
        let sender = tasks.remove(&task_id);
        if let Some(sender) = sender {
            info!("Found and removed task {} from pending_tasks", task_id);
            debug!(
                "Remaining pending task count after removal: {}",
                tasks.len()
            );
            let response = if success {
                Ok(CompletionResponse {
                    id: task_id.clone(),
                    object: "text_completion".to_string(),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    model: "gpuf-android".to_string(),
                    session_id: None,
                    client_id: None,
                    cache_status: None,
                    choices: vec![CompletionChoice {
                        text: result.unwrap_or_default(),
                        index: 0,
                        logprobs: None,
                        finish_reason: "stop".to_string(),
                    }],
                    usage: CompletionUsage {
                        prompt_tokens,
                        completion_tokens,
                        total_tokens: prompt_tokens.saturating_add(completion_tokens),
                        analysis_tokens: None,
                        final_tokens: None,
                    },
                })
            } else {
                Err(anyhow!("Inference failed: {}", error.unwrap_or_default()))
            };

            if let Err(_) = sender.send(response) {
                warn!("Failed to send result for task {}", task_id);
            }
        } else {
            {
                let mut partial = self.partial_results.lock().await;
                partial.remove(&task_id);
            }

            // This commonly happens when the SSE client disconnects and we cancel/remove the
            // stream sender before the device finishes sending its final chunks.
            debug!(
                "Dropping inference result for task {} because it is no longer pending (likely canceled/disconnected). Previously pending count: {}",
                task_id,
                pending_count_before
            );
        }
    }

    /// Select best Android device for inference
    async fn select_best_device(
        &self,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<ClientId> {
        let clients = self.active_clients.lock().await;

        let mut best_device: Option<(ClientId, u16)> = None;
        let mut device_count = 0;

        let mut consider_device =
            |client_id: &ClientId, client_info: &crate::handle::ClientInfo| {
                // Only consider authenticated Android devices
                if !client_info.authed {
                    return;
                }

                // Check if device has system info (Android devices should have this)
                let Some(system_info) = &client_info.system_info else {
                    return;
                };

                // Simple load balancing: choose device with lowest CPU + Memory usage
                let total_load: u16 = (system_info.cpu_usage + system_info.memory_usage) as u16;
                device_count += 1;

                if best_device.is_none() || total_load < best_device.as_ref().unwrap().1 {
                    best_device = Some((*client_id, total_load));
                }
            };

        match allowed_client_ids {
            Some(allowed) => {
                // Base set = allowed ids; lookup active client info from map (O(1) average)
                for client_id in allowed {
                    if let Some(client_info) = clients.get(client_id) {
                        consider_device(client_id, client_info);
                    }
                }
            }
            None => {
                // No restriction; base set = all active clients
                for (client_id, client_info) in clients.iter() {
                    consider_device(client_id, client_info);
                }
            }
        }

        if let Some((client_id, _load)) = best_device {
            info!(
                "Selected device {} for inference (load: {}%, available devices: {})",
                client_id.log_label(),
                _load,
                device_count
            );
            Ok(client_id)
        } else {
            Err(anyhow!("No available Android devices found"))
        }
    }

    /// Send inference task to device
    async fn send_task_to_device(
        &self,
        device_id: &ClientId,
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
    ) -> Result<()> {
        use common::write_command;

        // Find active client connection
        let mut clients = self.active_clients.lock().await;
        let client_info = clients
            .get_mut(device_id)
            .ok_or_else(|| anyhow!("Device not found or not connected"))?;

        // Check if client is authenticated and ready
        if !client_info.authed {
            error!("Device {} not authenticated", device_id.log_label());
            return Err(anyhow!("Device not authenticated"));
        }

        // Try to acquire writer lock (non-blocking to avoid deadlocks)
        let mut writer = client_info
            .writer
            .try_lock()
            .map_err(|_| anyhow!("Device is busy, please try again"))?;

        // Create and send inference task command
        let inference_task = CommandV1::InferenceTask {
            task_id: task_id.clone(),
            session_id,
            cache_policy,
            prompt,
            max_tokens,
            temperature,
            top_k,
            top_p,
            repeat_penalty,
            repeat_last_n,
            min_keep,
        };

        let command = Command::V1(inference_task);
        info!(
            "sent inference task {} to device {} (prompt_bytes={}, max_tokens={})",
            task_id,
            device_id.log_label(),
            match &command {
                Command::V1(CommandV1::InferenceTask { prompt, .. }) => prompt.len(),
                _ => 0,
            },
            max_tokens
        );
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;

        info!(
            "Successfully sent inference task {} to device {}",
            task_id,
            device_id.log_label()
        );
        Ok(())
    }

    /// Execute inference task
    pub async fn execute_inference(
        &self,
        request: CompletionRequest,
        routing: Option<SessionRouting>,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<(CompletionResponse, SessionRouteOutcome)> {
        let task_id = Uuid::new_v4().to_string();

        // Create response channel
        let (sender, receiver) = oneshot::channel();
        {
            let mut tasks = self.pending_tasks.lock().await;
            let existing_task_count = tasks.len();
            debug!(
                "Existing pending task count before insert: {}",
                existing_task_count
            );
            tasks.insert(task_id.clone(), sender);
            info!(
                "Stored task {} in pending_tasks (total: {})",
                task_id,
                tasks.len()
            );
            debug!("All pending task count after insert: {}", tasks.len());
        }

        let model_name = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
        let route_outcome = self
            .select_device_for_request(
                allowed_client_ids,
                routing.as_ref(),
                Some(model_name.as_str()),
                false,
            )
            .await?;
        let (session_id, cache_policy) = session_command_fields(routing.as_ref());

        // Send task to device
        info!(
            "About to send task {} to device {}",
            task_id,
            route_outcome.client_id.log_label()
        );
        if let Err(e) = self
            .send_task_to_device(
                &route_outcome.client_id,
                task_id.clone(),
                session_id,
                cache_policy,
                request.prompt,
                request.max_tokens.unwrap_or(1024),
                request.temperature.unwrap_or(0.7),
                request.top_k.unwrap_or(40),
                request.top_p.unwrap_or(0.9),
                request.repeat_penalty.unwrap_or(1.1),
                request.repeat_last_n.unwrap_or(64),
                request.min_keep.unwrap_or(1),
            )
            .await
        {
            // Clean up pending task on failure
            let mut tasks = self.pending_tasks.lock().await;
            tasks.remove(&task_id);
            error!(
                "Failed to send inference task to device {}: {}",
                route_outcome.client_id.log_label(),
                e
            );
            return Err(e);
        }

        info!(
            "Task {} sent successfully, now waiting for result...",
            task_id
        );

        // Check if task is still in pending_tasks before waiting
        {
            let tasks = self.pending_tasks.lock().await;
            info!("Pending tasks count before timeout wait: {}", tasks.len());
            if !tasks.contains_key(&task_id) {
                error!("Task {} missing from pending_tasks before wait!", task_id);
                return Err(anyhow!(
                    "Task {} was removed from pending_tasks unexpectedly",
                    task_id
                ));
            }
        }

        // Wait for result with timeout
        let timeout_secs: u64 = std::env::var("GPUF_INFERENCE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(300);

        info!(
            "Waiting for result of task {} with {}s timeout...",
            task_id, timeout_secs
        );
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), receiver).await {
            Ok(Ok(response)) => {
                info!("Task {} completed successfully", task_id);
                response.map(|response| (response, route_outcome))
            }
            Ok(Err(_)) => {
                warn!("Task {} response channel closed", task_id);
                Err(anyhow!("Task response channel closed"))
            }
            Err(_) => {
                // Clean up pending task on timeout
                let mut tasks = self.pending_tasks.lock().await;
                tasks.remove(&task_id);
                warn!("Task {} timed out after {} seconds", task_id, timeout_secs);
                Err(anyhow!(
                    "Inference task timed out after {} seconds",
                    timeout_secs
                ))
            }
        }
    }

    /// Get list of available devices
    pub async fn get_available_devices(
        &self,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Vec<DeviceInfo> {
        let clients = self.active_clients.lock().await;
        let mut devices = Vec::new();

        let mut maybe_push_device =
            |client_id: &ClientId, client_info: &crate::handle::ClientInfo| {
                if !client_info.authed {
                    return;
                }
                let device = DeviceInfo {
                    client_id: hex::encode(&client_id.0),
                    status: if client_info.system_info.is_some() {
                        "online".to_string()
                    } else {
                        "initializing".to_string()
                    },
                    cpu_usage: client_info
                        .system_info
                        .as_ref()
                        .map(|s| s.cpu_usage)
                        .unwrap_or(0),
                    memory_usage: client_info
                        .system_info
                        .as_ref()
                        .map(|s| s.memory_usage)
                        .unwrap_or(0),
                    device_count: client_info.devices_info.len() as u32,
                };
                devices.push(device);
            };

        match allowed_client_ids {
            Some(allowed) => {
                for client_id in allowed {
                    if let Some(client_info) = clients.get(client_id) {
                        maybe_push_device(client_id, client_info);
                    }
                }
            }
            None => {
                for (client_id, client_info) in clients.iter() {
                    maybe_push_device(client_id, client_info);
                }
            }
        }

        devices
    }
}

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub client_id: String,
    pub status: String,
    pub cpu_usage: u8,
    pub memory_usage: u8,
    pub device_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::ClientInfo;
    use common::{DevicesInfo, Model, COMMAND_V1_BASE_VERSION};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn client(seed: u8) -> ClientId {
        ClientId([seed; 16])
    }

    fn test_client_info(version: u32, os_type: OsType, engine_type: EngineType) -> ClientInfo {
        let mut device = DevicesInfo::default();
        device.os_type = os_type.clone();
        device.engine_type = engine_type;

        ClientInfo {
            connection_id: 1,
            writer: Arc::new(Mutex::new(Box::new(tokio::io::sink()))),
            authed: true,
            version,
            os_type,
            system_info: Some(crate::handle::SystemInfo {
                cpu_usage: 10,
                memory_usage: 20,
                disk_usage: 30,
                device_memsize: 16,
                total_tflops: 1,
                last_heartbeat: std::time::SystemTime::now(),
                memsize_gb: 16,
            }),
            devices_info: vec![device],
            connected_at: chrono::Utc::now(),
            models: Some(vec![Model {
                id: "bge-m3-q8_0".to_string(),
                object: "model".to_string(),
                created: 0,
                owned_by: "gpuf".to_string(),
            }]),
        }
    }

    fn routing(owner: &str, policy: CachePolicy) -> SessionRouting {
        SessionRouting::new(
            "session-1234567890".to_string(),
            owner.to_string(),
            policy,
            Some("model-a".to_string()),
            false,
        )
    }

    #[test]
    fn validate_session_id_rejects_short_or_unsafe_values() {
        assert!(validate_session_id("short").is_err());
        assert!(validate_session_id("session-1234567890").is_ok());
        assert!(validate_session_id("session-1234567890/../../x").is_err());
    }

    #[test]
    fn cache_policy_parser_accepts_expected_values() {
        assert_eq!(CachePolicy::parse(None).unwrap(), CachePolicy::Auto);
        assert_eq!(
            CachePolicy::parse(Some("bypass")).unwrap(),
            CachePolicy::Bypass
        );
        assert_eq!(
            CachePolicy::parse(Some("RESET")).unwrap(),
            CachePolicy::Reset
        );
        assert!(CachePolicy::parse(Some("force")).is_err());
    }

    #[test]
    fn embedding_support_requires_new_non_mobile_llama_worker() {
        let supported = test_client_info(
            COMMAND_V1_EMBEDDING_TASKS_VERSION,
            OsType::LINUX,
            EngineType::Llama,
        );
        assert!(InferenceScheduler::client_supports_embedding_tasks(
            &supported
        ));

        let old_worker =
            test_client_info(COMMAND_V1_BASE_VERSION, OsType::LINUX, EngineType::Llama);
        assert!(!InferenceScheduler::client_supports_embedding_tasks(
            &old_worker
        ));

        let mobile = test_client_info(
            COMMAND_V1_EMBEDDING_TASKS_VERSION,
            OsType::ANDROID,
            EngineType::Llama,
        );
        assert!(!InferenceScheduler::client_supports_embedding_tasks(
            &mobile
        ));

        let non_llama = test_client_info(
            COMMAND_V1_EMBEDDING_TASKS_VERSION,
            OsType::LINUX,
            EngineType::Ollama,
        );
        assert!(!InferenceScheduler::client_supports_embedding_tasks(
            &non_llama
        ));
    }

    #[tokio::test]
    async fn execute_embedding_rejects_result_count_mismatch() {
        let active_clients = Arc::new(Mutex::new(HashMap::new()));
        active_clients.lock().await.insert(
            client(1),
            test_client_info(
                COMMAND_V1_EMBEDDING_TASKS_VERSION,
                OsType::LINUX,
                EngineType::Llama,
            ),
        );

        let scheduler = Arc::new(InferenceScheduler::new(active_clients));
        let scheduler_for_task = scheduler.clone();
        let request_task = tokio::spawn(async move {
            scheduler_for_task
                .execute_embedding(
                    EmbeddingRequest {
                        model: "bge-m3-q8_0".to_string(),
                        input: EmbeddingInput::Batch(vec![
                            "hello".to_string(),
                            "world".to_string(),
                        ]),
                        encoding_format: Some("float".to_string()),
                        normalize: true,
                    },
                    None,
                )
                .await
        });

        let task_id = loop {
            let maybe_task_id = {
                let tasks = scheduler.pending_embedding_tasks.lock().await;
                tasks.keys().next().cloned()
            };
            if let Some(task_id) = maybe_task_id {
                break task_id;
            }
            tokio::task::yield_now().await;
        };

        scheduler
            .handle_embedding_result(task_id, true, vec![vec![1.0, 2.0]], None, 2)
            .await;

        let err = request_task
            .await
            .expect("embedding task should not panic")
            .expect_err("mismatched embedding count must fail");
        assert!(err.to_string().contains("result count mismatch"));
    }

    #[test]
    fn worker_session_id_is_owner_scoped_and_redacted() {
        let owner_a = "owner-a";
        let owner_b = "owner-b";
        let external_session = "session-1234567890";

        let scoped_a = worker_scoped_session_id(owner_a, external_session);
        let scoped_b = worker_scoped_session_id(owner_b, external_session);

        assert_eq!(scoped_a.len(), 64);
        assert_ne!(scoped_a, scoped_b);
        assert!(!scoped_a.contains(external_session));
        assert!(!scoped_a.contains(owner_a));

        let route = routing(owner_a, CachePolicy::Auto);
        let (session_id, cache_policy) = session_command_fields(Some(&route));
        assert_eq!(session_id.as_deref(), Some(scoped_a.as_str()));
        assert_eq!(cache_policy.as_deref(), Some("auto"));
    }

    #[test]
    fn same_session_reuses_bound_worker() {
        let mut table = SessionRouteTable::default();
        let now = Instant::now();
        let worker = client(7);
        let route = routing("owner-a", CachePolicy::Auto);

        table.bind(&route, worker, now);

        assert_eq!(
            table.resolve(&route, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::Use {
                client_id: worker,
                cache_status: SessionCacheStatus::Hit
            }
        );
    }

    #[test]
    fn different_owner_cannot_reuse_session_route() {
        let mut table = SessionRouteTable::default();
        let now = Instant::now();
        table.bind(&routing("owner-a", CachePolicy::Auto), client(1), now);

        let err = table
            .resolve(
                &routing("owner-b", CachePolicy::Auto),
                Some(&[client(1)]),
                now,
            )
            .unwrap_err();
        assert!(err.to_string().contains("session owner mismatch"));
    }

    #[test]
    fn reset_removes_owned_route_and_bypass_does_not_bind() {
        let mut table = SessionRouteTable::default();
        let now = Instant::now();
        let worker = client(3);
        let auto = routing("owner-a", CachePolicy::Auto);
        table.bind(&auto, worker, now);

        let reset = routing("owner-a", CachePolicy::Reset);
        assert_eq!(
            table.resolve(&reset, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::SelectCold(SessionCacheStatus::Reset)
        );
        assert_eq!(
            table.resolve(&auto, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::SelectCold(SessionCacheStatus::Cold)
        );

        let bypass = routing("owner-a", CachePolicy::Bypass);
        table.bind(&bypass, worker, now);
        assert_eq!(
            table.resolve(&auto, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::SelectCold(SessionCacheStatus::Cold)
        );
    }

    #[test]
    fn stale_or_disallowed_routes_do_not_silently_cross_owner_boundary() {
        let mut table = SessionRouteTable::default();
        let now = Instant::now();
        let worker = client(4);
        let route = routing("owner-a", CachePolicy::Auto);
        table.bind(
            &route,
            worker,
            now - SESSION_ROUTE_TTL - Duration::from_secs(1),
        );

        assert_eq!(
            table.resolve(&route, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::SelectCold(SessionCacheStatus::Evicted)
        );

        table.bind(&route, worker, now);
        let err = table.resolve(&route, Some(&[client(5)]), now).unwrap_err();
        assert!(err.to_string().contains("no longer allowed"));
    }

    #[test]
    fn model_change_invalidates_route() {
        let mut table = SessionRouteTable::default();
        let now = Instant::now();
        let worker = client(9);
        table.bind(&routing("owner-a", CachePolicy::Auto), worker, now);

        let mut next = routing("owner-a", CachePolicy::Auto);
        next.model_id = Some("model-b".to_string());
        assert_eq!(
            table.resolve(&next, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::SelectCold(SessionCacheStatus::Evicted)
        );
    }

    #[test]
    fn route_table_evicts_lru_when_capacity_is_reached() {
        let mut table = SessionRouteTable::new(2, SESSION_ROUTE_TTL);
        let now = Instant::now();

        let route1 = routing("owner-a", CachePolicy::Auto);
        let route2 = SessionRouting::new(
            "session-2234567890".to_string(),
            "owner-a".to_string(),
            CachePolicy::Auto,
            Some("model-a".to_string()),
            false,
        );
        let route3 = SessionRouting::new(
            "session-3234567890".to_string(),
            "owner-a".to_string(),
            CachePolicy::Auto,
            Some("model-a".to_string()),
            false,
        );

        table.bind(&route1, client(1), now - Duration::from_secs(10));
        table.bind(&route2, client(2), now - Duration::from_secs(5));
        table.bind(&route3, client(3), now);

        let snapshot = table.snapshot();
        assert_eq!(snapshot.routes_current, 2);
        assert_eq!(snapshot.routes_max, 2);
        assert_eq!(snapshot.sticky_route_bind_total, 3);
        assert!(snapshot.sticky_route_eviction_total >= 1);

        let err = table.resolve(&route1, Some(&[client(1)]), now).unwrap();
        assert_eq!(
            err,
            SessionRouteDecision::SelectCold(SessionCacheStatus::Cold)
        );
    }

    #[test]
    fn snapshot_reports_hit_bypass_and_reset_counts() {
        let mut table = SessionRouteTable::default();
        let now = Instant::now();
        let worker = client(8);
        let route = routing("owner-a", CachePolicy::Auto);
        table.bind(&route, worker, now);

        assert_eq!(
            table.resolve(&route, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::Use {
                client_id: worker,
                cache_status: SessionCacheStatus::Hit
            }
        );

        let bypass = routing("owner-a", CachePolicy::Bypass);
        assert_eq!(
            table.resolve(&bypass, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::SelectCold(SessionCacheStatus::Bypass)
        );

        let reset = routing("owner-a", CachePolicy::Reset);
        assert_eq!(
            table.resolve(&reset, Some(&[worker]), now).unwrap(),
            SessionRouteDecision::SelectCold(SessionCacheStatus::Reset)
        );

        let snapshot = table.snapshot();
        assert!(snapshot.sticky_route_hit_total >= 1);
        assert!(snapshot.sticky_route_bypass_total >= 1);
        assert!(snapshot.sticky_route_reset_total >= 1);
    }
}
