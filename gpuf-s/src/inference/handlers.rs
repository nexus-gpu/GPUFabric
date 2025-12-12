use axum::{
    extract::{State, Path},
    http::StatusCode,
    Json,
};
use serde_json::Value;
use std::sync::Arc;
use tracing::{error, info};

use crate::inference::{
    gateway::InferenceGateway,
    scheduler::{CompletionRequest, ChatCompletionRequest, CompletionResponse, ChatCompletionResponse, ModelInfo, DeviceInfo},
};

// OpenAI Compatible API Handlers

/// Handle text completion requests
pub async fn handle_completion(
    State(gateway): State<Arc<InferenceGateway>>,
    Json(request): Json<CompletionRequest>,
) -> Result<Json<CompletionResponse>, StatusCode> {
    info!("Received completion request: {} chars", request.prompt.len());
    
    match gateway.scheduler.execute_inference(request).await {
        Ok(response) => {
            info!("Completion request completed successfully");
            Ok(Json(response))
        },
        Err(e) => {
            error!("Completion request failed: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Handle chat completion requests
pub async fn handle_chat_completion(
    State(gateway): State<Arc<InferenceGateway>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, StatusCode> {
    info!("Received chat completion request with {} messages", request.messages.len());
    
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

    match gateway.scheduler.execute_inference(completion_request).await {
        Ok(response) => {
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
                usage: response.usage,
            };
            
            info!("Chat completion request completed successfully");
            Ok(Json(chat_response))
        },
        Err(e) => {
            error!("Chat completion request failed: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
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
) -> Json<Vec<DeviceInfo>> {
    let devices = gateway.scheduler.get_available_devices().await;
    Json(devices)
}

/// Get device status by ID
pub async fn get_device_status(
    State(gateway): State<Arc<InferenceGateway>>,
    Path(device_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let devices = gateway.scheduler.get_available_devices().await;
    
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
