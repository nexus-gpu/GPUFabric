use axum::{
    extract::{State, Path, Extension},
    http::{StatusCode, HeaderMap},
    Json,
    response::{IntoResponse, Response},
};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{error, info, debug};

use crate::inference::{
    gateway::{InferenceGateway, AuthContext},
    scheduler::{CompletionRequest, ChatCompletionRequest, ChatCompletionResponse, ModelInfo, DeviceInfo},
};

// OpenAI Compatible API Handlers

/// Handle text completion requests
pub async fn handle_completion(
    State(gateway): State<Arc<InferenceGateway>>,
    Extension(auth): Extension<AuthContext>,
    headers: HeaderMap,
    Json(request): Json<CompletionRequest>,
) -> Response {
    info!("Received completion request: {} chars", request.prompt.len());
    
    // Extract Request-ID header
    let request_id = headers
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    debug!("Request-ID: {:?}", request_id);
    
    match gateway
        .scheduler
        .execute_inference(request, Some(auth.client_ids.as_slice()))
        .await
    {
        Ok(response) => {
            // Send metrics to Kafka if needed
            if auth.access_level != -1 {
                if  let Some(chosen_client_id) = auth.client_ids.first() {
                if let Err(e) = gateway.send_request_metrics(
                    request_id,
                    *chosen_client_id,
                    auth.access_level,
                ).await {
                    error!("Failed to send request metrics: {}", e);
                    // Don't fail the request, just log the error
                }
            }
            }
       
            
            info!("Completion request completed successfully");
            Json(response).into_response()
        },
        Err(e) => {
            error!("Completion request failed: {}", e);
            // Return appropriate HTTP status code with JSON error message
            let (status, error_message) = if e.to_string().contains("No available Android devices found") {
                (
                    StatusCode::SERVICE_UNAVAILABLE, // 503 - No devices available
                    "No available Android devices found. Please ensure at least one device is online and valid."
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR, // 500 - Other errors
                    "Internal server error occurred while processing the request."
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
    info!("Received chat completion request with {} messages", request.messages.len());
    
    // Extract Request-ID header
    let request_id = headers
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    debug!("Request-ID: {:?}", request_id);
    
    // Convert chat messages to a single prompt
    let prompt = request.messages
        .iter()
        .map(|msg| format!("{}: {}", msg.role, msg.content))
        .collect::<Vec<_>>()
        .join("\n");

    let completion_request = CompletionRequest {
        prompt,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        top_k: request.top_k,
        top_p: request.top_p,
        repeat_penalty: request.repeat_penalty,
        model: request.model,
        stream: request.stream,
    };

    match gateway
        .scheduler
        .execute_inference(completion_request, Some(auth.client_ids.as_slice()))
        .await
    {
        Ok(response) => {
            // Send metrics to Kafka if needed
            if auth.access_level != -1 {
                if let Some(chosen_client_id) = auth.client_ids.first() {
                    if let Err(e) = gateway.send_request_metrics(
                            request_id,
                            *chosen_client_id,
                            auth.access_level,
                        ).await {
                            error!("Failed to send request metrics: {}", e);
                            // Don't fail the request, just log the error
                        }
                }
            }
            // Convert completion response to chat completion format
            let chat_response = ChatCompletionResponse {
                id: response.id,
                object: "chat.completion".to_string(),
                created: response.created,
                model: response.model,
                choices: response.choices.into_iter().map(|choice| {
                    crate::inference::scheduler::ChatCompletionChoice {
                        index: choice.index,
                        message: crate::inference::scheduler::ChatMessage {
                            role: "assistant".to_string(),
                            content: choice.text,
                        },
                        finish_reason: choice.finish_reason,
                    }
                }).collect(),
                //usage: response.usage,
            };
            
            info!("Chat completion request completed successfully");
            Json(chat_response).into_response()
        },
        Err(e) => {
            error!("Chat completion request failed: {}", e);
            // Return appropriate HTTP status code with JSON error message
            let (status, error_message) = if e.to_string().contains("No available Android devices found") {
                (
                    StatusCode::SERVICE_UNAVAILABLE, // 503 - No devices available
                    "No available Android devices found. Please ensure at least one device is online and valid."
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR, // 500 - Other errors
                    "Internal server error occurred while processing the request."
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

/// List available models
pub async fn list_models() -> Json<Vec<ModelInfo>> {
    let models = vec![
        ModelInfo {
            id: "gpuf-android".to_string(),
            object: "model".to_string(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            owned_by: "gpuf".to_string(),
        }
    ];
    
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
