use crate::api_server::ApiServer;
use crate::db::models;
use crate::util::msg::ApiResponse;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::error;
use validator::Validate;

// Request/Response types for model management
#[derive(Debug, Deserialize, Validate)]
pub struct CreateOrUpdateModelRequest {
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub engine_type: i16,
    pub is_active: Option<bool>,
    pub min_memory_mb: Option<i32>,
    pub min_gpu_memory_gb: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ModelResponse {
    pub id: i32,
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub is_active: bool,
    pub min_memory_mb: Option<i32>,
    pub min_gpu_memory_gb: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// Create or update a model
pub async fn create_or_update_model(
    State(app_state): State<Arc<ApiServer>>,
    Json(payload): Json<CreateOrUpdateModelRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    // Validate input
    if payload.name.is_empty() || payload.version.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    match models::create_or_update_model(
        &app_state.db_pool,
        &payload.name,
        &payload.version,
        payload.version_code,
        payload.engine_type,
        payload.is_active,
        payload.min_memory_mb,
        payload.min_gpu_memory_gb,
    )
    .await
    {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            error!("Failed to create/update model: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// Get all models with optional filtering
pub async fn get_models(
    State(app_state): State<Arc<ApiServer>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<Vec<ModelResponse>>>, StatusCode> {
    let is_active = params.get("is_active").and_then(|s| s.parse::<bool>().ok());
    let min_gpu_memory_gb = params
        .get("min_gpu_memory_gb")
        .and_then(|s| s.parse::<i32>().ok());

    match models::get_models_list(&app_state.db_pool, is_active, None, min_gpu_memory_gb).await {
        Ok(models) => {
            let models = models
                .into_iter()
                .map(|model| ModelResponse {
                    id: model.id,
                    name: model.name,
                    version: model.version,
                    version_code: model.version_code,
                    is_active: model.is_active,
                    min_memory_mb: model.min_memory_mb,
                    min_gpu_memory_gb: model.min_gpu_memory_gb,
                    created_at: model.created_at,
                })
                .collect();
            Ok(Json(ApiResponse::success(models)))
        }
        Err(e) => {
            error!("Failed to get models: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
