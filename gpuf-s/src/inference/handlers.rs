use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{sse::Event, sse::Sse, IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info};

use crate::inference::{
    gateway::{AuthContext, InferenceGateway},
    scheduler::{
        ChatCompletionRequest, ChatCompletionResponse, CompletionRequest, DeviceInfo, ModelInfo,
        StreamEvent,
    },
};
use crate::util::protoc::ClientId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelFamily {
    Llama3Instruct,
    LegacyHashPrompt,
    ChatMLLike,
}

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

fn should_force_short_answer(messages: &[crate::inference::scheduler::ChatMessage]) -> bool {
    let last_user = messages.iter().rev().find(|m| m.role == "user");
    let Some(m) = last_user else {
        return false;
    };
    let c = m.content.to_ascii_lowercase();
    c.contains("只回复")
        || c.contains("只回答")
        || c.contains("仅回复")
        || c.contains("only reply")
        || c.contains("only respond")
        || c.contains("reply only")
}

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

    // Extract Request-ID header
    let request_id = headers
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    debug!("Request-ID: {:?}", request_id);

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
        if auth.access_level == -1 {
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

    if request.stream.unwrap_or(false) {
        let max_tokens_effective: u32 = request.max_tokens.unwrap_or(4090);
        let model_name = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
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
            .execute_inference_stream(request, Some(allowed_ids))
            .await;

        match stream_res {
            Ok((task_id, device_id, rx)) => {
                if auth.access_level == -1 {
                    let gateway = gateway.clone();
                    let request_id = request_id.clone();
                    let access_level = auth.access_level;
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
                    device_id,
                    finished: finished.clone(),
                });
                let scheduler = gateway.scheduler.clone();
                let stop_state: Arc<Mutex<StopMarkerState>> =
                    Arc::new(Mutex::new(StopMarkerState::new(&[])));
                let s = ReceiverStream::new(rx)
                    .then(move |ev| {
                        let guard = guard.clone();
                        let stop_state = stop_state.clone();
                        let scheduler = scheduler.clone();
                        let task_id = task_id.clone();
                        let model_name = model_name.clone();
                        let finished = finished.clone();
                        async move {
                            let _guard = guard;
                            let data = match ev {
                                StreamEvent::Delta(text) => {
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

                return Sse::new(s).into_response();
            }
            Err(e) => {
                error!("Completion request failed: {}", e);
                let error_response = json!({
                    "error": {"message": e.to_string(), "type": "api_error", "code": 500}
                });
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response();
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
        .execute_inference(request, Some(allowed_ids))
        .await
    {
        Ok(response) => {
            // Send metrics to Kafka if needed
            if auth.access_level == -1 {
                if let Some(chosen_client_id) = auth.client_ids.first() {
                    if let Err(e) = gateway
                        .send_request_metrics(request_id, *chosen_client_id, auth.access_level)
                        .await
                    {
                        error!("Failed to send request metrics: {}", e);
                        // Don't fail the request, just log the error
                    }
                }
            }

            let mut response = response;
            let finish_reason = if response.usage.completion_tokens >= max_tokens_effective {
                "length"
            } else {
                "stop"
            };

            if let Some(choice) = response.choices.get_mut(0) {
                choice.finish_reason = finish_reason.to_string();
            }

            info!("Completion request completed successfully");
            Json(response).into_response()
        }
        Err(e) => {
            error!("Completion request failed: {}", e);
            // Return appropriate HTTP status code with JSON error message
            let (status, error_message) = if e
                .to_string()
                .contains("No available Android devices found")
            {
                (
                    StatusCode::SERVICE_UNAVAILABLE, // 503 - No devices available
                    "No available Android devices found. Please ensure at least one device is online and valid."
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR, // 500 - Other errors
                    "Internal server error occurred while processing the request.",
                )
            };

            let error_response = json!({
                "error": {
                    "message": error_message,
                    "type": "api_error",
                    "code": status.as_u16()
                }
            });

            (status, Json(error_response)).into_response()
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

    // Extract Request-ID header
    let request_id = headers
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    debug!("Request-ID: {:?}", request_id);

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
        if auth.access_level == -1 {
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

    if request.stream.unwrap_or(false) {
        let max_tokens_effective: u32 = request.max_tokens.unwrap_or(4090);
        let model_name = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
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
                Some(allowed_ids),
            )
            .await;

        match stream_res {
            Ok((task_id, device_id, rx)) => {
                if auth.access_level == -1 {
                    let gateway = gateway.clone();
                    let request_id = request_id.clone();
                    let access_level = auth.access_level;
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
                    device_id,
                    finished: finished.clone(),
                });
                let scheduler = gateway.scheduler.clone();
                let stop_state: Arc<Mutex<StopMarkerState>> =
                    Arc::new(Mutex::new(StopMarkerState::new(&[])));
                let s = ReceiverStream::new(rx)
                    .then(move |ev| {
                        let guard = guard.clone();
                        let stop_state = stop_state.clone();
                        let scheduler = scheduler.clone();
                        let task_id = task_id.clone();
                        let model_name = model_name.clone();
                        let finished = finished.clone();
                        async move {
                            let _guard = guard;
                            let data = match ev {
                                StreamEvent::Delta(text) => {
                                    let text = {
                                        let mut st = stop_state.lock().await;
                                        let (out, _hit_stop) = st.consume(&text);
                                        out
                                    };

                                    if text.is_empty() {
                                        return None;
                                    }

                                    let delta = json!({"role": "assistant", "content": text});
                                    let payload = json!({
                                        "id": task_id,
                                        "object": "chat.completion.chunk",
                                        "created": created,
                                        "model": model_name,
                                        "choices": [{
                                            "index": 0,
                                            "delta": delta,
                                            "finish_reason": null
                                        }]
                                    });
                                    payload.to_string()
                                }
                                StreamEvent::Finish(usage) => {
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

                return Sse::new(s).into_response();
            }
            Err(e) => {
                error!("Chat completion request failed: {}", e);
                let error_response = json!({
                    "error": {"message": e.to_string(), "type": "api_error", "code": 500}
                });
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response();
            }
        }
    }

    let model_name = request.model.clone().unwrap_or_else(|| "gpuf".to_string());
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
            Some(allowed_ids),
        )
        .await;

    match stream_res {
        Ok((task_id, device_id, mut rx)) => {
            if auth.access_level == -1 {
                let gateway = gateway.clone();
                let request_id = request_id.clone();
                let access_level = auth.access_level;
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

            while let Some(ev) = rx.recv().await {
                match ev {
                    StreamEvent::Delta(d) => {
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
                        break;
                    }
                }
            }

            let usage = usage_final.unwrap_or(crate::inference::scheduler::CompletionUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            });
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
                choices: vec![crate::inference::scheduler::ChatCompletionChoice {
                    index: 0,
                    message: crate::inference::scheduler::ChatMessage {
                        role: "assistant".to_string(),
                        content: text,
                    },
                    finish_reason: finish_reason.to_string(),
                }],
                usage,
            };

            Json(chat_response).into_response()
        }
        Err(e) => {
            error!("Chat completion request failed: {}", e);
            let error_response = json!({
                "error": {"message": e.to_string(), "type": "api_error", "code": 500}
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
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
