use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::util::msg::ApiResponse;
use crate::util::protoc::ClientId;
use std::sync::Arc;
use tracing::{error, info};

use crate::api_server::ApiServer;
use crate::api_server::ClientInfoResponse;
use crate::db::stats::{ClientHeartbeatInfo, ClientMonitorInfo};
use crate::db::{
    client::{self, ClientDeviceDetailResponse, ClientDeviceInfo},
    stats::{self, EditClientRequest},
};

// Create Client Request
#[derive(Debug, Validate, Serialize, Deserialize)]
pub struct CreateClientRequest {
    #[validate(length(min = 1, max = 32))]
    pub user_id: String,
    #[validate(length(min = 1, max = 32))]
    pub client_id: String,
    pub client_status: String,
    #[validate(length(min = 1, max = 64))]
    pub os_type: Option<String>,
    #[validate(length(min = 1, max = 32))]
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct ClientListResponse {
    pub total: usize,
    pub devices: Vec<ClientDeviceInfo>,
}

//#@ get_user_clients api
#[derive(Debug, Deserialize)]
pub struct ClientListQuery {
    pub user_id: String,
    pub client_id: Option<String>,
    pub status: Option<String>,
    pub name: Option<String>,
    pub valid_status: Option<String>,
}

// API Handlers
pub async fn insert_client(
    State(app_state): State<Arc<ApiServer>>,
    Json(payload): Json<CreateClientRequest>,
) -> Result<Json<ApiResponse<Vec<ClientInfoResponse>>>, StatusCode> {
    if payload.user_id.is_empty() || payload.client_id.is_empty() {
        error!("Invalid user_id or client_id");
        return Err(StatusCode::BAD_REQUEST);
    }
    info!(
        "Inserting client for user_id_len={} client_id_len={}",
        payload.user_id.len(),
        payload.client_id.len()
    );

    let client_id = match payload.client_id.parse::<ClientId>() {
        Ok(client_id) => client_id,
        Err(e) => {
            error!("Failed to parse client ID: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    let _ = client::upsert_client_info(
        &app_state.db_pool,
        &payload.user_id,
        &client_id,
        &payload.os_type,
        &payload.client_status,
        &payload.name,
    )
    .await;
    Ok(Json(ApiResponse::success(vec![])))
}

// #get_user_clients
pub async fn get_user_clients(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ClientListQuery>,
) -> Result<Json<ApiResponse<ClientListResponse>>, StatusCode> {
    // Get database connection
    let mut devices = client::get_user_client_status_list(
        &app_state.db_pool,
        &query.user_id,
        query.client_id.as_ref(),
        query.status.as_ref(),
        query.name.as_ref(),
        query.valid_status.as_ref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get user clients: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let client_ids: Vec<String> = devices.iter().map(|d| d.client_id.clone()).collect();
    let models_map =
        client::get_loaded_models_batch_from_redis(&app_state.redis_client, &client_ids)
            .await
            .unwrap_or_default();
    for d in &mut devices {
        if let Some(models) = models_map.get(&d.client_id) {
            d.loaded_models = models.clone();
        }
    }
    let response = ClientListResponse {
        total: devices.len(),
        devices,
    };
    Ok(Json(ApiResponse::success(response)))
}

pub async fn get_user_client_status_list(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ClientListQuery>,
) -> Result<Json<ApiResponse<ClientListResponse>>, StatusCode> {
    let mut devices = client::get_user_client_status_list(
        &app_state.db_pool,
        &query.user_id,
        query.client_id.as_ref(),
        query.status.as_ref(),
        query.name.as_ref(),
        query.valid_status.as_ref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get user status clients: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let client_ids: Vec<String> = devices.iter().map(|d| d.client_id.clone()).collect();
    let models_map =
        client::get_loaded_models_batch_from_redis(&app_state.redis_client, &client_ids)
            .await
            .unwrap_or_default();
    for d in &mut devices {
        if let Some(models) = models_map.get(&d.client_id) {
            d.loaded_models = models.clone();
        }
    }
    let response = ClientListResponse {
        total: devices.len(),
        devices,
    };
    Ok(Json(ApiResponse::success(response)))
}

//#@ get_user_clients_device_detail api
#[derive(Debug, Deserialize)]
pub struct ClientDetailQuery {
    pub user_id: String,
    pub client_id: String,
    #[allow(dead_code)] // Optional query parameters for future filtering
    pub status: Option<String>,
    #[allow(dead_code)] // Optional query parameters for future filtering
    pub name: Option<String>,
}

pub async fn get_client_detail(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ClientDetailQuery>,
) -> Result<Json<ApiResponse<ClientDeviceDetailResponse>>, StatusCode> {
    // Get database connection
    let client_id_bytes = query
        .client_id
        .parse::<ClientId>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let devices =
        client::get_client_device_detail(&app_state.db_pool, &query.user_id, &client_id_bytes)
            .await
            .map_err(|e| {
                tracing::error!("/api/user/client_list: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    Ok(Json(ApiResponse::success(devices)))
}

// Edit client info handler
pub async fn edit_client_info(
    State(app_state): State<Arc<ApiServer>>,
    Json(payload): Json<EditClientRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    if payload.user_id.is_empty() || payload.client_id.is_empty() {
        error!("Missing required fields");
        return Err(StatusCode::BAD_REQUEST);
    }

    // Validate client_status if provided
    if let Some(status) = &payload.client_status {
        let valid_statuses = ["active", "online", "offline", "maintenance", "error"];
        if !valid_statuses.contains(&status.as_str()) {
            error!("Invalid client_status: {}", status);
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // Validate valid_status if provided
    if let Some(valid_status) = &payload.valid_status {
        if valid_status != "valid" && valid_status != "invalid" {
            error!("Invalid valid_status: {}", valid_status);
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    match stats::update_gpu_asset_status(&app_state.db_pool, &payload).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            error!("Failed to update client info: {}", e);
            Ok(Json(ApiResponse::<()>::error(e.to_string())))
        }
    }
}

// Request query parameters
#[derive(Debug, Deserialize)]
pub struct ClientStatQuery {
    pub user_id: String,
}

pub async fn get_client_stats(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ClientStatQuery>,
) -> Result<Json<ApiResponse<stats::ClientStatResponse>>, StatusCode> {
    // Get database connection
    let devices = stats::get_client_stats(
        &app_state.db_pool,
        &query.user_id,
        Some(time::Duration::minutes(2)),
        Some(time::Duration::hours(48)),
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get client stats: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(ApiResponse::success(devices)))
}

// Request query parameters
#[derive(Debug, Validate, Serialize, Deserialize)]
pub struct ClientMonitorQuery {
    #[validate(length(min = 1, max = 32))]
    pub user_id: String,
    pub client_id: Option<String>,
}

pub async fn get_client_monitor(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ClientMonitorQuery>,
) -> Result<Json<ApiResponse<Vec<ClientMonitorInfo>>>, StatusCode> {
    let devices_info =
        stats::get_client_monitor(&app_state.db_pool, &query.user_id, query.client_id)
            .await
            .map_err(|e| {
                tracing::error!("Failed to get client stats: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    Ok(Json(ApiResponse::success(devices_info)))
}

#[derive(Debug, Validate, Serialize, Deserialize)]
pub struct ClientHealthQuery {
    #[validate(length(min = 1, max = 32))]
    pub user_id: String,
    pub client_id: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

pub async fn get_client_health(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ClientHealthQuery>,
) -> Result<Json<ApiResponse<Vec<ClientHeartbeatInfo>>>, StatusCode> {
    let devices_info = stats::get_client_heartbeats(
        &app_state.db_pool,
        &query.user_id,
        query.client_id,
        query.start_date,
        query.end_date,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get client stats: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(ApiResponse::success(devices_info)))
}

// Model Download Progress Query
#[derive(Debug, Deserialize)]
pub struct ModelDownloadProgressQuery {
    pub client_id: String,
}

#[derive(Debug, Serialize)]
pub struct ModelDownloadProgressResponse {
    pub client_id: String,
    pub model_name: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub percentage: Option<f32>,
    pub speed_bps: Option<u64>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub timestamp: Option<i64>,
}

/// Get model download progress for a client
/// GET /api/user/model_download_progress?client_id=xxx
pub async fn get_model_download_progress(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ModelDownloadProgressQuery>,
) -> Result<Json<ApiResponse<ModelDownloadProgressResponse>>, StatusCode> {
    use redis::AsyncCommands;

    let mut conn = app_state
        .redis_client
        .get_async_connection()
        .await
        .map_err(|e| {
            error!("Failed to get Redis connection: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let key = format!("client:{}:model_download", query.client_id);

    // Try to get all fields from Redis hash
    let result: Result<Vec<(String, String)>, _> = conn.hgetall(&key).await;

    match result {
        Ok(fields) if !fields.is_empty() => {
            // Parse fields from Redis
            let mut response = ModelDownloadProgressResponse {
                client_id: query.client_id.clone(),
                model_name: None,
                downloaded_bytes: None,
                total_bytes: None,
                percentage: None,
                speed_bps: None,
                status: None,
                error: None,
                timestamp: None,
            };

            for (field, value) in fields {
                match field.as_str() {
                    "model_name" => response.model_name = Some(value),
                    "downloaded_bytes" => response.downloaded_bytes = value.parse().ok(),
                    "total_bytes" => response.total_bytes = value.parse().ok(),
                    "percentage" => response.percentage = value.parse().ok(),
                    "speed_bps" => response.speed_bps = value.parse().ok(),
                    "status" => response.status = Some(value),
                    "error" => response.error = Some(value),
                    "timestamp" => response.timestamp = value.parse().ok(),
                    _ => {}
                }
            }

            info!(
                "Model download progress fetched (client_id_len={}, model_present={}, status_present={}, error_present={})",
                query.client_id.len(),
                response.model_name.is_some(),
                response.status.is_some(),
                response.error.is_some()
            );
            Ok(Json(ApiResponse::success(response)))
        }
        _ => {
            // No download in progress
            let response = ModelDownloadProgressResponse {
                client_id: query.client_id.clone(),
                model_name: None,
                downloaded_bytes: None,
                total_bytes: None,
                percentage: None,
                speed_bps: None,
                status: None,
                error: None,
                timestamp: None,
            };
            Ok(Json(ApiResponse::success(response)))
        }
    }
}
