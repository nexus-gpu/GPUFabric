use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use std::sync::Arc;

use crate::api_server::ApiServer;
use crate::db::compute_map;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeMapSummary {
    pub online_nodes: u32,
    pub total_tflops: u32,
    pub token_tps: f64,
    pub today_token_total: u64,
    pub today_token_unit: String,
    pub used_nodes: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeMapNode {
    pub id: String,
    pub name: String,
    pub lng: f64,
    pub lat: f64,
    pub node_count: u32,
    pub tflops: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ComputeMapLink {
    pub from: String,
    pub to: String,
    pub value: u32,
}

#[derive(Debug, Serialize)]
pub struct ComputeMapResponse {
    pub summary: Option<ComputeMapSummary>,
    pub nodes: Vec<ComputeMapNode>,
    pub links: Vec<ComputeMapLink>,
}

pub async fn get_compute_map(
    State(app_state): State<Arc<ApiServer>>,
) -> Result<Json<ComputeMapResponse>, StatusCode> {
    let response = compute_map::get_compute_map(&app_state.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get compute map: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(response))
}
