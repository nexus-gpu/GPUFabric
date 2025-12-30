use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::error;
use validator::Validate;

use crate::api_server::ApiServer;
use crate::db::apk;
use crate::util::msg::ApiResponse;

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertApkRequest {
    #[validate(length(min = 1, max = 255))]
    pub package_name: String,
    #[validate(length(min = 1, max = 64))]
    pub version_name: String,
    pub version_code: i64,
    #[validate(length(min = 1))]
    pub download_url: String,

    pub channel: Option<String>,
    pub min_os_version: Option<String>,
    pub sha256: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub is_active: Option<bool>,
    pub released_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ApkResponse {
    pub id: i64,
    pub package_name: String,
    pub version_name: String,
    pub version_code: i64,
    pub download_url: String,
    pub channel: Option<String>,
    pub min_os_version: Option<String>,
    pub sha256: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub is_active: bool,
    pub released_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<apk::ApkVersion> for ApkResponse {
    fn from(v: apk::ApkVersion) -> Self {
        Self {
            id: v.id,
            package_name: v.package_name,
            version_name: v.version_name,
            version_code: v.version_code,
            download_url: v.download_url,
            channel: v.channel,
            min_os_version: v.min_os_version,
            sha256: v.sha256,
            file_size_bytes: v.file_size_bytes,
            is_active: v.is_active,
            released_at: v.released_at,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}

pub async fn upsert_apk(
    State(app_state): State<Arc<ApiServer>>,
    Json(payload): Json<UpsertApkRequest>,
) -> Result<Json<ApiResponse<ApkResponse>>, StatusCode> {
    payload.validate().map_err(|_| StatusCode::BAD_REQUEST)?;
    if payload.package_name.is_empty()
        || payload.version_name.is_empty()
        || payload.download_url.is_empty()
        || payload.version_code <= 0
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let record = apk::upsert_apk_version(
        &app_state.db_pool,
        &payload.package_name,
        &payload.version_name,
        payload.version_code,
        &payload.download_url,
        payload.channel.as_deref(),
        payload.min_os_version.as_deref(),
        payload.sha256.as_deref(),
        payload.file_size_bytes,
        payload.is_active,
        payload.released_at,
    )
    .await
    .map_err(|e| {
        error!("Failed to upsert apk: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(ApiResponse::success(record.into())))
}

pub async fn get_apk(
    State(app_state): State<Arc<ApiServer>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<Option<ApkResponse>>>, StatusCode> {
    let package_name = params
        .get("package_name")
        .map(|s| s.as_str())
        .ok_or(StatusCode::BAD_REQUEST)?;

    let version_code = params
        .get("version_code")
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    let record = apk::get_apk_version(&app_state.db_pool, package_name, version_code)
        .await
        .map_err(|e| {
            error!("Failed to get apk: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(ApiResponse::success(record.map(Into::into))))
}

pub async fn list_apk(
    State(app_state): State<Arc<ApiServer>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<Vec<ApkResponse>>>, StatusCode> {
    let package_name = params.get("package_name").map(|s| s.as_str());
    let channel = params.get("channel").map(|s| s.as_str());
    let is_active = params.get("is_active").and_then(|s| s.parse::<bool>().ok());
    let limit = params.get("limit").and_then(|s| s.parse::<u32>().ok());

    let records =
        apk::list_apk_versions(&app_state.db_pool, package_name, channel, is_active, limit)
            .await
            .map_err(|e| {
                error!("Failed to list apk: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    Ok(Json(ApiResponse::success(
        records.into_iter().map(Into::into).collect(),
    )))
}
