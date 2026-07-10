use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{sse::Event, sse::Sse, IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info};

use crate::db::token_usage::{insert_token_usage, TokenUsageInsert};
use crate::inference::{
    gateway::{AuthContext, InferenceGateway},
    scheduler::{
        validate_session_id, CachePolicy, ChatCompletionRequest, ChatCompletionResponse,
        CompletionRequest, CompletionUsage, DeviceInfo, EmbeddingInput, EmbeddingRequest,
        ModelInfo, P2PResponseInfo, SessionRouteOutcome, SessionRouting, SophnetEmbeddingRequest,
        SophnetEmbeddingResponse, SophnetEmbeddingUsage, StreamEvent,
    },
};
use crate::util::protoc::ClientId;
use common::OutputPhase;

#[cfg(feature = "experimental")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelFamily {
    Llama3Instruct,
    LegacyHashPrompt,
    ChatMLLike,
}

#[cfg(feature = "experimental")]
fn detect_model_family(model_name: &str) -> ModelFamily {
    let m = model_name.to_ascii_lowercase();
    if m.contains("llama3") || m.contains("llama-3") || m.contains("llama_3") {
        return ModelFamily::Llama3Instruct;
    }
    if m.contains("deepseek") || m.contains("gpt") || m.contains("chatgpt") || m.contains("openai")
    {
        return ModelFamily::ChatMLLike;
    }
    ModelFamily::LegacyHashPrompt
}

#[cfg(feature = "experimental")]
fn stop_markers_for_family(family: ModelFamily) -> &'static [&'static str] {
    match family {
        ModelFamily::Llama3Instruct => &["<|eot_id|>", "\n\n###"],
        ModelFamily::ChatMLLike => &[
            "<|end|>",
            "<|start|>",
            "<|channel|>",
            "<|call|>",
            "<|tool|>",
            "<|im_end|>",
            "<|im_start|>",
            "\n\n###",
        ],
        ModelFamily::LegacyHashPrompt => &["\n\n###"],
    }
}

#[cfg(feature = "experimental")]
fn should_force_short_answer(messages: &[crate::inference::scheduler::ChatMessage]) -> bool {
    let last_user = messages.iter().rev().find(|m| m.role == "user");
    let Some(m) = last_user else {
        return false;
    };
    let Some(content) = m.content.as_text() else {
        return false;
    };
    let c = content.to_ascii_lowercase();
    c.contains("only reply")
        || c.contains("only answer")
        || c.contains("reply only")
        || c.contains("only reply")
        || c.contains("only respond")
        || c.contains("reply only")
}

#[cfg(feature = "experimental")]
fn role_to_chatml(role: &str) -> &str {
    match role {
        "system" => "system",
        "user" => "user",
        "assistant" => "assistant",
        _ => "user",
    }
}

struct StreamCancelGuard {
    scheduler: Arc<crate::inference::InferenceScheduler>,
    task_id: String,
    device_id: ClientId,
    finished: Arc<AtomicBool>,
}

struct StopMarkerState {
    stopped: bool,
    carry: String,
    markers: &'static [&'static str],
}

impl StopMarkerState {
    fn new(markers: &'static [&'static str]) -> Self {
        Self {
            stopped: false,
            carry: String::new(),
            markers,
        }
    }

    fn flush(&mut self) -> String {
        std::mem::take(&mut self.carry)
    }

    fn consume(&mut self, text: &str) -> (String, bool) {
        if self.stopped {
            return (String::new(), true);
        }

        let combined = if self.carry.is_empty() {
            text.to_string()
        } else {
            let mut s = std::mem::take(&mut self.carry);
            s.push_str(text);
            s
        };

        let mut stop_at: Option<usize> = None;
        for m in self.markers {
            if let Some(idx) = combined.find(m) {
                stop_at = Some(stop_at.map(|cur| cur.min(idx)).unwrap_or(idx));
            }
        }
        if let Some(idx) = stop_at {
            self.stopped = true;
            return (combined[..idx].to_string(), true);
        }

        let keep = self
            .markers
            .iter()
            .map(|m| m.len())
            .max()
            .unwrap_or(0)
            .saturating_sub(1);
        if combined.len() > keep {
            let mut split_at = combined.len() - keep;
            while split_at > 0 && !combined.is_char_boundary(split_at) {
                split_at -= 1;
            }
            let (out, tail) = combined.split_at(split_at);
            self.carry = tail.to_string();
            (out.to_string(), false)
        } else {
            self.carry = combined;
            (String::new(), false)
        }
    }
}

impl Drop for StreamCancelGuard {
    fn drop(&mut self) {
        if self.finished.load(Ordering::SeqCst) {
            return;
        }
        let scheduler = self.scheduler.clone();
        let task_id = self.task_id.clone();
        let device_id = self.device_id;
        tokio::spawn(async move {
            let _ = scheduler.cancel_inference(&task_id, &device_id).await;
        });
    }
}

// OpenAI Compatible API Handlers

fn json_error(status: StatusCode, message: impl Into<String>, error_type: &str) -> Response {
    let error_response = json!({
        "error": {
            "message": message.into(),
            "type": error_type,
            "code": status.as_u16()
        }
    });
    (status, Json(error_response)).into_response()
}

fn optional_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn request_id_from_headers(headers: &HeaderMap) -> Option<String> {
    optional_header(headers, "request-id")
        .or_else(|| optional_header(headers, "x-request-id"))
        .map(str::to_string)
}

fn chat_usage_endpoint_for_request(request: &ChatCompletionRequest) -> &'static str {
    let multimodal = request
        .messages
        .iter()
        .any(|message| message.content.is_multimodal());
    if !multimodal {
        return "chat.completion";
    }

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
}

fn parse_session_id(
    headers: &HeaderMap,
    body_session_id: Option<&str>,
) -> Result<Option<String>, Response> {
    let header_session_id = optional_header(headers, "x-gpuf-session-id");
    let body_session_id = body_session_id.map(str::trim).filter(|s| !s.is_empty());

    match (header_session_id, body_session_id) {
        (None, None) => Ok(None),
        (Some(raw), None) | (None, Some(raw)) => validate_session_id(raw).map(Some).map_err(|e| {
            json_error(
                StatusCode::BAD_REQUEST,
                e.to_string(),
                "invalid_request_error",
            )
        }),
        (Some(header_raw), Some(body_raw)) => {
            let header = validate_session_id(header_raw).map_err(|e| {
                json_error(
                    StatusCode::BAD_REQUEST,
                    e.to_string(),
                    "invalid_request_error",
                )
            })?;
            let body = validate_session_id(body_raw).map_err(|e| {
                json_error(
                    StatusCode::BAD_REQUEST,
                    e.to_string(),
                    "invalid_request_error",
                )
            })?;
            if header != body {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "x-gpuf-session-id and body session_id must match",
                    "invalid_request_error",
                ));
            }
            Ok(Some(header))
        }
    }
}

fn parse_cache_policy(
    headers: &HeaderMap,
    body_cache_policy: Option<&str>,
) -> Result<CachePolicy, Response> {
    let header_policy = optional_header(headers, "x-gpuf-cache-policy");
    let body_policy = body_cache_policy.map(str::trim).filter(|s| !s.is_empty());

    let parsed = match (header_policy, body_policy) {
        (None, None) => CachePolicy::Auto,
        (Some(raw), None) | (None, Some(raw)) => CachePolicy::parse(Some(raw)).map_err(|e| {
            json_error(
                StatusCode::BAD_REQUEST,
                e.to_string(),
                "invalid_request_error",
            )
        })?,
        (Some(header_raw), Some(body_raw)) => {
            let header = CachePolicy::parse(Some(header_raw)).map_err(|e| {
                json_error(
                    StatusCode::BAD_REQUEST,
                    e.to_string(),
                    "invalid_request_error",
                )
            })?;
            let body = CachePolicy::parse(Some(body_raw)).map_err(|e| {
                json_error(
                    StatusCode::BAD_REQUEST,
                    e.to_string(),
                    "invalid_request_error",
                )
            })?;
            if header != body {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "x-gpuf-cache-policy and body cache_policy must match",
                    "invalid_request_error",
                ));
            }
            header
        }
    };

    Ok(parsed)
}

fn session_routing_for_request(
    headers: &HeaderMap,
    body_session_id: Option<&str>,
    body_cache_policy: Option<&str>,
    model_id: Option<String>,
    auth: &AuthContext,
    explicit_target: bool,
) -> Result<Option<SessionRouting>, Response> {
    let session_id = parse_session_id(headers, body_session_id)?;
    let cache_policy = parse_cache_policy(headers, body_cache_policy)?;

    let Some(session_id) = session_id else {
        if cache_policy != CachePolicy::Auto {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "cache_policy requires session_id or x-gpuf-session-id",
                "invalid_request_error",
            ));
        }
        return Ok(None);
    };

    Ok(Some(SessionRouting::new(
        session_id,
        format!("bearer:{}", auth.token_hash),
        cache_policy,
        model_id,
        explicit_target,
    )))
}

fn scheduler_error_response(error: &anyhow::Error) -> Response {
    let message = error.to_string();
    let status = if message.contains("session owner mismatch")
        || message.contains("no longer allowed")
    {
        StatusCode::FORBIDDEN
    } else if message.contains("unsupported encoding_format") || message.contains("embedding input")
    {
        StatusCode::BAD_REQUEST
    } else if message.contains("No available Android devices found")
        || message.contains("No compatible client found")
        || message.contains("No non-mobile embedding-capable client found")
    {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };

    json_error(status, message, "api_error")
}

fn apply_route_headers(response: &mut Response, outcome: &SessionRouteOutcome) {
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&outcome.client_id.to_string()) {
        headers.insert(HeaderName::from_static("x-gpuf-client-id"), value);
    }
    if let Some(session_id) = &outcome.session_id {
        if let Ok(value) = HeaderValue::from_str(session_id) {
            headers.insert(HeaderName::from_static("x-gpuf-session-id"), value);
        }
    }
    if let Some(status) = outcome.cache_status {
        headers.insert(
            HeaderName::from_static("x-gpuf-cache-status"),
            HeaderValue::from_static(status.as_str()),
        );
    }
}

fn response_with_route_headers(mut response: Response, outcome: &SessionRouteOutcome) -> Response {
    apply_route_headers(&mut response, outcome);
    response
}

fn target_client_id_for_request(
    headers: &HeaderMap,
    auth: &AuthContext,
) -> Result<Option<ClientId>, Response> {
    let target_client_id = match headers
        .get("x-target-client-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(raw) => match crate::util::protoc::ClientId::from_str(raw) {
            Ok(id) => Some(id),
            Err(e) => {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Invalid x-target-client-id: {}", e),
                    "invalid_request_error",
                ));
            }
        },
    };

    if let Some(target) = target_client_id {
        if auth.access_level.is_metered() {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "x-target-client-id is not allowed for metered tokens",
                "forbidden",
            ));
        }

        if !auth.client_ids.contains(&target) {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "x-target-client-id is not in the allowed client_ids for this token",
                "forbidden",
            ));
        }
    }

    Ok(target_client_id)
}

fn allowed_client_ids_for_request<'a>(
    target_client_id: &'a Option<ClientId>,
    auth: &'a AuthContext,
) -> &'a [ClientId] {
    target_client_id
        .as_ref()
        .map(std::slice::from_ref)
        .unwrap_or(auth.client_ids.as_slice())
}

fn gpuf_model_for_sophnet_embedding(model: Option<&str>) -> Result<String, Response> {
    let default_model = std::env::var("GPUF_DEFAULT_EMBEDDING_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "bge-m3-Q8_0".to_string());

    let Some(model) = model.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(default_model);
    };

    if model.eq_ignore_ascii_case("clip-embeddings") {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "clip-embeddings is not supported by this GPUFabric embedding gateway yet",
            "invalid_request_error",
        ));
    }

    if model.eq_ignore_ascii_case("text-embeddings") || model.eq_ignore_ascii_case("bge-m3") {
        return Ok(default_model);
    }

    Ok(model.to_string())
}

fn sophnet_embedding_to_gpuf_request(
    request: SophnetEmbeddingRequest,
) -> Result<EmbeddingRequest, Response> {
    if request.easyllm_id.trim().is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "easyllm_id must not be empty",
            "invalid_request_error",
        ));
    }

    if request.input_images.as_ref().map_or(false, |images| {
        images.iter().any(|image| !image.trim().is_empty())
    }) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "input_images is not supported by this GPUFabric embedding gateway yet",
            "invalid_request_error",
        ));
    }

    if request.dimensions != 1024 {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "only dimensions=1024 is supported for bge-m3 embeddings",
            "invalid_request_error",
        ));
    }

    if let Some(encoding_type) = request.encoding_type.as_deref() {
        if !encoding_type.eq_ignore_ascii_case("float") {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                format!(
                    "unsupported encoding_type '{}', only 'float' is supported",
                    encoding_type
                ),
                "invalid_request_error",
            ));
        }
    }

    Ok(EmbeddingRequest {
        model: gpuf_model_for_sophnet_embedding(request.model.as_deref())?,
        input: EmbeddingInput::Batch(request.input_texts),
        encoding_format: Some("float".to_string()),
        normalize: request.normalized.unwrap_or(true),
    })
}

fn apply_completion_route_metadata(
    response: &mut crate::inference::scheduler::CompletionResponse,
    outcome: &SessionRouteOutcome,
) {
    response.session_id = outcome.session_id.clone();
    response.client_id = Some(outcome.client_id);
    response.cache_status = outcome.cache_status;
}

fn apply_chat_route_metadata(response: &mut ChatCompletionResponse, outcome: &SessionRouteOutcome) {
    response.session_id = outcome.session_id.clone();
    response.client_id = Some(outcome.client_id);
    response.cache_status = outcome.cache_status;
}

fn normalize_completion_usage_for_response(usage: &mut CompletionUsage) {
    if usage.completion_tokens == 0 {
        return;
    }

    if matches!(usage.final_tokens, Some(value) if value > 0) {
        return;
    }

    let analysis_tokens = usage
        .analysis_tokens
        .unwrap_or(0)
        .min(usage.completion_tokens);
    usage.final_tokens = Some(usage.completion_tokens.saturating_sub(analysis_tokens));
}

async fn record_token_usage(
    gateway: &InferenceGateway,
    auth: &AuthContext,
    request_id: Option<String>,
    outcome: &SessionRouteOutcome,
    model: String,
    endpoint: &'static str,
    usage: &CompletionUsage,
    stream: bool,
) {
    if usage.total_tokens == 0 {
        return;
    }

    if let Err(err) = insert_token_usage(
        &gateway.db_pool,
        TokenUsageInsert {
            request_id,
            token_hash: Some(auth.token_hash.clone()),
            client_id: outcome.client_id,
            model,
            endpoint: endpoint.to_string(),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            success: true,
            stream,
        },
    )
    .await
    {
        error!("Failed to record token usage: {}", err);
    }
}

/// Handle text completion requests
pub async fn handle_completion(
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(auth): Extension<AuthContext>,
    headers: HeaderMap,
    Json(request): Json<CompletionRequest>,
) -> Response {
    info!(
        "Received completion request: {} chars",
        request.prompt.len()
    );

    let request_id = request_id_from_headers(&headers);

    debug!("Request-ID present: {}", request_id.is_some());

    let target_client_id = match headers
        .get("x-target-client-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(raw) => match crate::util::protoc::ClientId::from_str(raw) {
            Ok(id) => Some(id),
            Err(e) => {
                let error_response = json!({
                    "error": {
                        "message": format!("Invalid x-target-client-id: {}", e),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                });
                return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
            }
        },
    };

    if let Some(target) = target_client_id {
        if auth.access_level.is_metered() {
            let error_response = json!({
                "error": {
                    "message": "x-target-client-id is not allowed for metered tokens",
                    "type": "forbidden",
                    "code": 403
                }
            });
            return (StatusCode::FORBIDDEN, Json(error_response)).into_response();
        }

        if !auth.client_ids.contains(&target) {
            let error_response = json!({
                "error": {
                    "message": "x-target-client-id is not in the allowed client_ids for this token",
                    "type": "forbidden",
                    "code": 403
                }
            });
            return (StatusCode::FORBIDDEN, Json(error_response)).into_response();
        }
    }

    let model_name = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
    let session_routing = match session_routing_for_request(
        &headers,
        request.session_id.as_deref(),
        request.cache_policy.as_deref(),
        Some(model_name.clone()),
        &auth,
        target_client_id.is_some(),
    ) {
        Ok(routing) => routing,
        Err(response) => return response,
    };

    if request.stream.unwrap_or(false) {
        let max_tokens_effective: u32 = request.max_tokens.unwrap_or(4090);
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let allowed_ids = target_client_id
            .as_ref()
            .map(std::slice::from_ref)
            .unwrap_or(auth.client_ids.as_slice());

        let stream_res = gateway
            .scheduler
            .execute_inference_stream(request, session_routing.clone(), Some(allowed_ids))
            .await;

        match stream_res {
            Ok((task_id, route_outcome, rx)) => {
                if auth.access_level.is_metered() {
                    let gateway = gateway.clone();
                    let request_id = request_id.clone();
                    let access_level = auth.access_level;
                    let device_id = route_outcome.client_id;
                    tokio::spawn(async move {
                        if let Err(e) = gateway
                            .send_request_metrics(request_id, device_id, access_level)
                            .await
                        {
                            error!("Failed to send request metrics: {}", e);
                        }
                    });
                }

                let finished = Arc::new(AtomicBool::new(false));
                let guard = Arc::new(StreamCancelGuard {
                    scheduler: gateway.scheduler.clone(),
                    task_id: task_id.clone(),
                    device_id: route_outcome.client_id,
                    finished: finished.clone(),
                });
                let stop_state: Arc<Mutex<StopMarkerState>> =
                    Arc::new(Mutex::new(StopMarkerState::new(&[])));
                let usage_gateway = gateway.clone();
                let usage_auth = auth.clone();
                let usage_request_id = request_id.clone();
                let usage_outcome = route_outcome.clone();
                let s = ReceiverStream::new(rx)
                    .then(move |ev| {
                        let guard = guard.clone();
                        let stop_state = stop_state.clone();
                        let task_id = task_id.clone();
                        let model_name = model_name.clone();
                        let finished = finished.clone();
                        let usage_gateway = usage_gateway.clone();
                        let usage_auth = usage_auth.clone();
                        let usage_request_id = usage_request_id.clone();
                        let usage_outcome = usage_outcome.clone();
                        async move {
                            let _guard = guard;
                            let data = match ev {
                                StreamEvent::Delta(text, _phase) => {
                                    let text = {
                                        let mut st = stop_state.lock().await;
                                        let (out, _hit_stop) = st.consume(&text);
                                        out
                                    };

                                    if text.is_empty() {
                                        return None;
                                    }
                                    let payload = json!({
                                        "id": task_id,
                                        "object": "text_completion",
                                        "created": created,
                                        "model": model_name,
                                        "choices": [{
                                            "index": 0,
                                            "text": text,
                                            "finish_reason": null
                                        }]
                                    });
                                    payload.to_string()
                                }
                                StreamEvent::Finish(usage) => {
                                    if let Some(usage_value) = usage.as_ref() {
                                        record_token_usage(
                                            &usage_gateway,
                                            &usage_auth,
                                            usage_request_id.clone(),
                                            &usage_outcome,
                                            model_name.clone(),
                                            "completion",
                                            usage_value,
                                            true,
                                        )
                                        .await;
                                    }
                                    let tail = {
                                        let mut st = stop_state.lock().await;
                                        if st.stopped {
                                            String::new()
                                        } else {
                                            st.flush()
                                        }
                                    };
                                    let finish_reason = usage
                                        .as_ref()
                                        .filter(|u| u.completion_tokens >= max_tokens_effective)
                                        .map(|_| "length")
                                        .unwrap_or("stop");
                                    let payload = json!({
                                        "id": task_id,
                                        "object": "text_completion",
                                        "created": created,
                                        "model": model_name,
                                        "choices": [{
                                            "index": 0,
                                            "text": tail,
                                            "finish_reason": finish_reason
                                        }],
                                        "usage": usage
                                    });
                                    payload.to_string()
                                }
                                StreamEvent::Error(msg) => {
                                    let payload = json!({
                                        "error": {"message": msg, "type": "api_error", "code": 500}
                                    });
                                    payload.to_string()
                                }
                                StreamEvent::Done => {
                                    finished.store(true, Ordering::SeqCst);
                                    "[DONE]".to_string()
                                }
                            };
                            Some(Ok::<Event, std::convert::Infallible>(
                                Event::default().data(data),
                            ))
                        }
                    })
                    .filter_map(|ev| async move { ev });

                return response_with_route_headers(Sse::new(s).into_response(), &route_outcome);
            }
            Err(e) => {
                error!("Completion request failed: {}", e);
                return scheduler_error_response(&e);
            }
        }
    }

    let max_tokens_effective: u32 = request.max_tokens.unwrap_or(1024);

    let allowed_ids = target_client_id
        .as_ref()
        .map(std::slice::from_ref)
        .unwrap_or(auth.client_ids.as_slice());

    match gateway
        .scheduler
        .execute_inference(request, session_routing, Some(allowed_ids))
        .await
    {
        Ok((response, route_outcome)) => {
            // Send metrics to Kafka if needed
            if auth.access_level.is_metered() {
                if let Err(e) = gateway
                    .send_request_metrics(
                        request_id.clone(),
                        route_outcome.client_id,
                        auth.access_level,
                    )
                    .await
                {
                    error!("Failed to send request metrics: {}", e);
                    // Don't fail the request, just log the error
                }
            }

            let mut response = response;
            record_token_usage(
                &gateway,
                &auth,
                request_id.clone(),
                &route_outcome,
                response.model.clone(),
                "completion",
                &response.usage,
                false,
            )
            .await;
            apply_completion_route_metadata(&mut response, &route_outcome);
            let finish_reason = if response.usage.completion_tokens >= max_tokens_effective {
                "length"
            } else {
                "stop"
            };

            if let Some(choice) = response.choices.get_mut(0) {
                choice.finish_reason = finish_reason.to_string();
            }

            info!("Completion request completed successfully");
            response_with_route_headers(Json(response).into_response(), &route_outcome)
        }
        Err(e) => {
            error!("Completion request failed: {}", e);
            scheduler_error_response(&e)
        }
    }
}

/// Handle chat completion requests
pub async fn handle_chat_completion(
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(auth): Extension<AuthContext>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> Response {
    info!(
        "Received chat completion request with {} messages",
        request.messages.len()
    );

    let request_id = request_id_from_headers(&headers);

    debug!("Request-ID present: {}", request_id.is_some());

    let target_client_id = match headers
        .get("x-target-client-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(raw) => match crate::util::protoc::ClientId::from_str(raw) {
            Ok(id) => Some(id),
            Err(e) => {
                let error_response = json!({
                    "error": {
                        "message": format!("Invalid x-target-client-id: {}", e),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                });
                return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
            }
        },
    };

    if let Some(target) = target_client_id {
        if auth.access_level.is_metered() {
            let error_response = json!({
                "error": {
                    "message": "x-target-client-id is not allowed for metered tokens",
                    "type": "forbidden",
                    "code": 403
                }
            });
            return (StatusCode::FORBIDDEN, Json(error_response)).into_response();
        }

        if !auth.client_ids.contains(&target) {
            let error_response = json!({
                "error": {
                    "message": "x-target-client-id is not in the allowed client_ids for this token",
                    "type": "forbidden",
                    "code": 403
                }
            });
            return (StatusCode::FORBIDDEN, Json(error_response)).into_response();
        }
    }

    let model_name = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
    let usage_endpoint = chat_usage_endpoint_for_request(&request);
    let session_routing = match session_routing_for_request(
        &headers,
        request.session_id.as_deref(),
        request.cache_policy.as_deref(),
        Some(model_name.clone()),
        &auth,
        target_client_id.is_some(),
    ) {
        Ok(routing) => routing,
        Err(response) => return response,
    };

    if request.stream.unwrap_or(false) {
        let max_tokens_effective: u32 = request.max_tokens.unwrap_or(4090);
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let allowed_ids = target_client_id
            .as_ref()
            .map(std::slice::from_ref)
            .unwrap_or(auth.client_ids.as_slice());
        debug!("Allowed client count: {}", allowed_ids.len());
        let stream_res = gateway
            .scheduler
            .execute_chat_inference_stream(
                model_name.clone(),
                request.messages.clone(),
                request.max_tokens.unwrap_or(4090),
                request.temperature.unwrap_or(0.7),
                request.top_k.unwrap_or(40),
                request.top_p.unwrap_or(0.9),
                request.repeat_penalty.unwrap_or(1.1),
                request.repeat_last_n.unwrap_or(64),
                request.min_keep.unwrap_or(1),
                session_routing.clone(),
                Some(allowed_ids),
            )
            .await;

        match stream_res {
            Ok((task_id, route_outcome, rx)) => {
                if auth.access_level.is_metered() {
                    let gateway = gateway.clone();
                    let request_id = request_id.clone();
                    let access_level = auth.access_level;
                    let device_id = route_outcome.client_id;
                    tokio::spawn(async move {
                        if let Err(e) = gateway
                            .send_request_metrics(request_id, device_id, access_level)
                            .await
                        {
                            error!("Failed to send request metrics: {}", e);
                        }
                    });
                }

                let finished = Arc::new(AtomicBool::new(false));
                let guard = Arc::new(StreamCancelGuard {
                    scheduler: gateway.scheduler.clone(),
                    task_id: task_id.clone(),
                    device_id: route_outcome.client_id,
                    finished: finished.clone(),
                });
                let stop_state: Arc<Mutex<StopMarkerState>> =
                    Arc::new(Mutex::new(StopMarkerState::new(&[])));
                let usage_gateway = gateway.clone();
                let usage_auth = auth.clone();
                let usage_request_id = request_id.clone();
                let usage_outcome = route_outcome.clone();
                let s = ReceiverStream::new(rx)
                    .then(move |ev| {
                        let guard = guard.clone();
                        let stop_state = stop_state.clone();
                        let task_id = task_id.clone();
                        let model_name = model_name.clone();
                        let finished = finished.clone();
                        let usage_gateway = usage_gateway.clone();
                        let usage_auth = usage_auth.clone();
                        let usage_request_id = usage_request_id.clone();
                        let usage_outcome = usage_outcome.clone();
                        async move {
                            let _guard = guard;
                            let data = match ev {
                                StreamEvent::Delta(text, phase) => {
                                    let text = {
                                        let mut st = stop_state.lock().await;
                                        let (out, _hit_stop) = st.consume(&text);
                                        out
                                    };

                                    if text.is_empty() {
                                        return None;
                                    }

                                    let delta = match phase {
                                        OutputPhase::Analysis => {
                                            json!({"role": "assistant", "reasoning_content": text})
                                        }
                                        _ => json!({"role": "assistant", "content": text}),
                                    };
                                    let payload = json!({
                                        "id": task_id,
                                        "object": "chat.completion.chunk",
                                        "created": created,
                                        "model": model_name,
                                        "client_id": usage_outcome.client_id,
                                        "p2p": {
                                            "enabled": false,
                                            "transport": "gateway",
                                            "fallback": false
                                        },
                                        "choices": [{
                                            "index": 0,
                                            "delta": delta,
                                            "finish_reason": null
                                        }]
                                    });
                                    payload.to_string()
                                }
                                StreamEvent::Finish(usage) => {
                                    let usage = usage.map(|mut usage| {
                                        normalize_completion_usage_for_response(&mut usage);
                                        usage
                                    });
                                    if let Some(usage_value) = usage.as_ref() {
                                        record_token_usage(
                                            &usage_gateway,
                                            &usage_auth,
                                            usage_request_id.clone(),
                                            &usage_outcome,
                                            model_name.clone(),
                                            usage_endpoint,
                                            usage_value,
                                            true,
                                        )
                                        .await;
                                    }
                                    let tail = {
                                        let mut st = stop_state.lock().await;
                                        if st.stopped {
                                            String::new()
                                        } else {
                                            st.flush()
                                        }
                                    };
                                    let finish_reason = usage
                                        .as_ref()
                                        .filter(|u| u.completion_tokens >= max_tokens_effective)
                                        .map(|_| "length")
                                        .unwrap_or("stop");

                                    let delta = if tail.is_empty() {
                                        json!({"role": "assistant"})
                                    } else {
                                        json!({"role": "assistant", "content": tail})
                                    };
                                    let payload = json!({
                                        "id": task_id,
                                        "object": "chat.completion.chunk",
                                        "created": created,
                                        "model": model_name,
                                        "client_id": usage_outcome.client_id,
                                        "p2p": {
                                            "enabled": false,
                                            "transport": "gateway",
                                            "fallback": false
                                        },
                                        "choices": [{
                                            "index": 0,
                                            "delta": delta,
                                            "finish_reason": finish_reason
                                        }],
                                        "usage": usage
                                    });
                                    payload.to_string()
                                }
                                StreamEvent::Error(msg) => {
                                    let payload = json!({
                                        "error": {"message": msg, "type": "api_error", "code": 500}
                                    });
                                    payload.to_string()
                                }
                                StreamEvent::Done => {
                                    finished.store(true, Ordering::SeqCst);
                                    "[DONE]".to_string()
                                }
                            };
                            Some(Ok::<Event, std::convert::Infallible>(
                                Event::default().data(data),
                            ))
                        }
                    })
                    .filter_map(|ev| async move { ev });

                return response_with_route_headers(Sse::new(s).into_response(), &route_outcome);
            }
            Err(e) => {
                error!("Chat completion request failed: {}", e);
                return scheduler_error_response(&e);
            }
        }
    }

    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let allowed_ids = target_client_id
        .as_ref()
        .map(std::slice::from_ref)
        .unwrap_or(auth.client_ids.as_slice());

    let stream_res = gateway
        .scheduler
        .execute_chat_inference_stream(
            model_name.clone(),
            request.messages.clone(),
            request.max_tokens.unwrap_or(4090),
            request.temperature.unwrap_or(0.7),
            request.top_k.unwrap_or(40),
            request.top_p.unwrap_or(0.9),
            request.repeat_penalty.unwrap_or(1.1),
            request.repeat_last_n.unwrap_or(64),
            request.min_keep.unwrap_or(1),
            session_routing,
            Some(allowed_ids),
        )
        .await;

    match stream_res {
        Ok((task_id, route_outcome, mut rx)) => {
            if auth.access_level.is_metered() {
                let gateway = gateway.clone();
                let request_id = request_id.clone();
                let access_level = auth.access_level;
                let device_id = route_outcome.client_id;
                tokio::spawn(async move {
                    if let Err(e) = gateway
                        .send_request_metrics(request_id, device_id, access_level)
                        .await
                    {
                        error!("Failed to send request metrics: {}", e);
                    }
                });
            }

            let mut text = String::new();
            let mut usage_final = None;
            let mut done_received = false;
            let timeout_secs = std::env::var("GPUF_CHAT_COMPLETION_TIMEOUT_SECS")
                .ok()
                .or_else(|| std::env::var("GPUF_INFERENCE_TIMEOUT_SECS").ok())
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|&v| v > 0)
                .unwrap_or(120);
            let timeout = tokio::time::sleep(Duration::from_secs(timeout_secs));
            tokio::pin!(timeout);

            while let Some(ev) = tokio::select! {
                ev = rx.recv() => ev,
                _ = &mut timeout => {
                    let _ = gateway
                        .scheduler
                        .cancel_inference(&task_id, &route_outcome.client_id)
                        .await;
                    let error_response = json!({
                        "error": {
                            "message": format!("Chat completion timed out after {} seconds", timeout_secs),
                            "type": "timeout_error",
                            "code": 504
                        }
                    });
                    return (StatusCode::GATEWAY_TIMEOUT, Json(error_response)).into_response();
                }
            } {
                match ev {
                    StreamEvent::Delta(d, _phase) => {
                        text.push_str(&d);
                    }
                    StreamEvent::Finish(usage) => {
                        usage_final = usage;
                    }
                    StreamEvent::Error(msg) => {
                        let error_response = json!({
                            "error": {"message": msg, "type": "api_error", "code": 500}
                        });
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response))
                            .into_response();
                    }
                    StreamEvent::Done => {
                        done_received = true;
                        break;
                    }
                }
            }

            if !done_received {
                let error_response = json!({
                    "error": {
                        "message": "Chat completion stream closed before completion",
                        "type": "api_error",
                        "code": 502
                    }
                });
                return (StatusCode::BAD_GATEWAY, Json(error_response)).into_response();
            }

            let mut usage = usage_final.unwrap_or(crate::inference::scheduler::CompletionUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                analysis_tokens: None,
                final_tokens: None,
            });
            normalize_completion_usage_for_response(&mut usage);
            let max_tokens_effective: u32 = request.max_tokens.unwrap_or(1024);
            let finish_reason = if usage.completion_tokens >= max_tokens_effective {
                "length"
            } else {
                "stop"
            };

            let chat_response = ChatCompletionResponse {
                id: task_id,
                object: "chat.completion".to_string(),
                created,
                model: model_name,
                session_id: None,
                client_id: None,
                cache_status: None,
                p2p: P2PResponseInfo::gateway(),
                choices: vec![crate::inference::scheduler::ChatCompletionChoice {
                    index: 0,
                    message: crate::inference::scheduler::ChatMessage {
                        role: "assistant".to_string(),
                        content: common::ChatMessageContent::Text(text),
                    },
                    finish_reason: finish_reason.to_string(),
                }],
                usage,
            };
            let mut chat_response = chat_response;
            record_token_usage(
                &gateway,
                &auth,
                request_id.clone(),
                &route_outcome,
                chat_response.model.clone(),
                usage_endpoint,
                &chat_response.usage,
                false,
            )
            .await;
            apply_chat_route_metadata(&mut chat_response, &route_outcome);

            response_with_route_headers(Json(chat_response).into_response(), &route_outcome)
        }
        Err(e) => {
            error!("Chat completion request failed: {}", e);
            scheduler_error_response(&e)
        }
    }
}

/// Handle embedding requests
pub async fn handle_embeddings(
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(auth): Extension<AuthContext>,
    headers: HeaderMap,
    Json(request): Json<EmbeddingRequest>,
) -> Response {
    info!("Received embedding request for model {}", request.model);

    let request_id = request_id_from_headers(&headers);

    let target_client_id = match target_client_id_for_request(&headers, &auth) {
        Ok(target_client_id) => target_client_id,
        Err(response) => return response,
    };
    let allowed_ids = allowed_client_ids_for_request(&target_client_id, &auth);

    match gateway
        .scheduler
        .execute_embedding(request, Some(allowed_ids))
        .await
    {
        Ok((response, route_outcome)) => {
            if auth.access_level.is_metered() {
                if let Err(e) = gateway
                    .send_request_metrics(
                        request_id.clone(),
                        route_outcome.client_id,
                        auth.access_level,
                    )
                    .await
                {
                    error!("Failed to send request metrics: {}", e);
                }
            }

            let usage = CompletionUsage {
                prompt_tokens: response.usage.prompt_tokens,
                completion_tokens: 0,
                total_tokens: response.usage.total_tokens,
                analysis_tokens: None,
                final_tokens: None,
            };
            record_token_usage(
                &gateway,
                &auth,
                request_id,
                &route_outcome,
                response.model.clone(),
                "embeddings",
                &usage,
                false,
            )
            .await;

            response_with_route_headers(Json(response).into_response(), &route_outcome)
        }
        Err(e) => {
            error!("Embedding request failed: {}", e);
            scheduler_error_response(&e)
        }
    }
}

pub async fn handle_sophnet_embeddings(
    Path(project_id): Path<String>,
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(auth): Extension<AuthContext>,
    headers: HeaderMap,
    Json(request): Json<SophnetEmbeddingRequest>,
) -> Response {
    if project_id.trim().is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "project_id must not be empty",
            "invalid_request_error",
        );
    }

    info!(
        "Received Sophnet embedding request for project {} easyllm {}",
        project_id, request.easyllm_id
    );

    let request_id = request_id_from_headers(&headers);

    let request = match sophnet_embedding_to_gpuf_request(request) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let target_client_id = match target_client_id_for_request(&headers, &auth) {
        Ok(target_client_id) => target_client_id,
        Err(response) => return response,
    };
    let allowed_ids = allowed_client_ids_for_request(&target_client_id, &auth);

    match gateway
        .scheduler
        .execute_embedding(request, Some(allowed_ids))
        .await
    {
        Ok((response, route_outcome)) => {
            if auth.access_level.is_metered() {
                if let Err(e) = gateway
                    .send_request_metrics(
                        request_id.clone(),
                        route_outcome.client_id,
                        auth.access_level,
                    )
                    .await
                {
                    error!("Failed to send request metrics: {}", e);
                }
            }

            let usage = CompletionUsage {
                prompt_tokens: response.usage.prompt_tokens,
                completion_tokens: 0,
                total_tokens: response.usage.total_tokens,
                analysis_tokens: None,
                final_tokens: None,
            };
            record_token_usage(
                &gateway,
                &auth,
                request_id,
                &route_outcome,
                response.model.clone(),
                "sophnet_embeddings",
                &usage,
                false,
            )
            .await;

            let sophnet_response = SophnetEmbeddingResponse {
                id: format!("embd-{}", uuid::Uuid::new_v4()),
                object: response.object,
                usage: SophnetEmbeddingUsage {
                    prompt_tokens: response.usage.prompt_tokens,
                    completion_tokens: None,
                    total_tokens: response.usage.total_tokens,
                    prompt_tokens_details: None,
                    completion_tokens_details: None,
                },
                data: response.data,
            };

            response_with_route_headers(Json(sophnet_response).into_response(), &route_outcome)
        }
        Err(e) => {
            error!("Sophnet embedding request failed: {}", e);
            scheduler_error_response(&e)
        }
    }
}

/// List available models
pub async fn list_models() -> Json<Vec<ModelInfo>> {
    let models = vec![ModelInfo {
        id: "gpuf-android".to_string(),
        object: "model".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        owned_by: "gpuf".to_string(),
    }];

    Json(models)
}

// Device Management API Handlers

/// List available devices
pub async fn list_devices(
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(auth): Extension<AuthContext>,
) -> Json<Vec<DeviceInfo>> {
    let devices = gateway
        .scheduler
        .get_available_devices(Some(auth.client_ids.as_slice()))
        .await;
    Json(devices)
}

pub async fn session_route_metrics(
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(_auth): Extension<AuthContext>,
) -> Json<crate::inference::scheduler::SessionRouteMetricsSnapshot> {
    Json(gateway.scheduler.session_route_metrics().await)
}

/// Get device status by ID
pub async fn get_device_status(
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(auth): Extension<AuthContext>,
    Path(device_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let devices = gateway
        .scheduler
        .get_available_devices(Some(auth.client_ids.as_slice()))
        .await;

    if let Some(device) = devices.into_iter().find(|d| d.client_id == device_id) {
        let status = serde_json::json!({
            "client_id": device.client_id,
            "status": device.status,
            "cpu_usage": device.cpu_usage,
            "memory_usage": device.memory_usage,
            "device_count": device.device_count,
            "last_updated": chrono::Utc::now().to_rfc3339()
        });
        Ok(Json(status))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{ChatContentPart, ChatMessageContent, ImageUrlValue};

    fn chat_request(model: &str, content: ChatMessageContent) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: Some(model.to_string()),
            messages: vec![crate::inference::scheduler::ChatMessage {
                role: "user".to_string(),
                content,
            }],
            max_tokens: None,
            temperature: None,
            top_k: None,
            top_p: None,
            repeat_penalty: None,
            repeat_last_n: None,
            min_keep: None,
            stream: None,
            session_id: None,
            cache_policy: None,
        }
    }

    fn image_content() -> ChatMessageContent {
        ChatMessageContent::Parts(vec![
            ChatContentPart {
                r#type: "text".to_string(),
                text: Some("extract text".to_string()),
                image_url: None,
                image: None,
            },
            ChatContentPart {
                r#type: "image_url".to_string(),
                text: None,
                image_url: Some(ImageUrlValue::Url("data:image/png;base64,AAAA".to_string())),
                image: None,
            },
        ])
    }

    #[test]
    fn chat_usage_endpoint_keeps_text_chat_completion() {
        let request = chat_request("gpuf", ChatMessageContent::Text("hello".to_string()));
        assert_eq!(chat_usage_endpoint_for_request(&request), "chat.completion");
    }

    #[test]
    fn chat_usage_endpoint_detects_ocr_image() {
        let request = chat_request("PaddleOCR-VL-1.6-GGUF", image_content());
        assert_eq!(chat_usage_endpoint_for_request(&request), "ocr.image");
    }

    #[test]
    fn chat_usage_endpoint_detects_generic_multimodal() {
        let request = chat_request("qwen-vl", image_content());
        assert_eq!(chat_usage_endpoint_for_request(&request), "multimodal.chat");
    }

    #[test]
    fn normalize_completion_usage_fills_missing_final_tokens() {
        let mut usage = CompletionUsage {
            prompt_tokens: 405,
            completion_tokens: 60,
            total_tokens: 465,
            analysis_tokens: Some(0),
            final_tokens: Some(0),
        };

        normalize_completion_usage_for_response(&mut usage);

        assert_eq!(usage.final_tokens, Some(60));
    }

    #[test]
    fn gateway_p2p_response_info_is_explicitly_disabled() {
        let value =
            serde_json::to_value(crate::inference::scheduler::P2PResponseInfo::gateway()).unwrap();

        assert_eq!(
            value,
            json!({
                "enabled": false,
                "transport": "gateway",
                "fallback": false
            })
        );
    }
}
