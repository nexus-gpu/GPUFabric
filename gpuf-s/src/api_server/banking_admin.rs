use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::api_server::ApiServer;
use crate::db::banking_admin;

#[derive(Debug, Serialize)]
pub struct BankingAdminEnvelope<T> {
    pub code: i32,
    pub message: String,
    pub data: T,
}

impl<T> BankingAdminEnvelope<T> {
    fn ok(data: T) -> Self {
        Self {
            code: 0,
            message: "ok".to_string(),
            data,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OverviewQuery {
    pub region: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewData {
    pub summary_cards: Vec<SummaryCard>,
    pub resource_usage: ResourceUsage,
    pub cluster_stack: Vec<ClusterStackItem>,
    pub status_breakdown: OverviewStatusBreakdown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryCard {
    pub key: String,
    pub label: String,
    pub value: Value,
    pub display_value: String,
    pub unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    pub total_devices: u32,
    pub used_devices: u32,
    pub usage_rate: u8,
}

#[derive(Debug, Serialize, Clone)]
pub struct ClusterStackItem {
    pub key: String,
    pub label: String,
    pub percent: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewStatusBreakdown {
    pub all: OverviewStatusMetrics,
    pub online: OverviewStatusMetrics,
    pub offline: OverviewStatusMetrics,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewStatusMetrics {
    pub nodes: u32,
    pub compute: u64,
    pub compute_display_value: String,
    pub compute_unit: String,
    pub resource_usage: ResourceUsage,
    pub cluster_stack: Vec<ClusterStackItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkMapData {
    pub cities: Vec<NetworkCity>,
    pub links: Vec<NetworkLink>,
    pub regions: Vec<NetworkRegion>,
    pub highlight_provinces: Vec<HighlightProvince>,
    pub top_cities: Vec<TopCity>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkMapQuery {
    /// Node inventory status filter: all, online, or offline. `status` is kept as an alias.
    pub node_status: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCity {
    pub id: String,
    pub name: String,
    pub province: String,
    pub coord: [f64; 2],
    pub nodes: u32,
    pub tflops: u32,
    pub gpu_model: String,
    pub tier: String,
    pub online_nodes: u32,
    pub used_nodes: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLink {
    pub id: String,
    pub from_city_id: String,
    pub to_city_id: String,
    pub value: u32,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRegion {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub city_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct HighlightProvince {
    pub name: String,
    pub level: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopCity {
    pub city_id: String,
    pub name: String,
    pub nodes: u32,
    pub tflops: u32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ComputeNodesQuery {
    pub status: Option<String>,
    pub device: Option<String>,
    pub region: Option<String>,
    pub keyword: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ComputeNodesData {
    pub items: Vec<ComputeNodeItem>,
    pub pagination: Pagination,
    pub stats: ComputeNodeStats,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeNodeItem {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub region: String,
    pub region_id: Option<String>,
    pub device: String,
    pub status: String,
    pub gpu: String,
    pub gpu_model: Option<String>,
    pub gpu_count: Option<u32>,
    pub load: u8,
    pub tokens_per_second: f64,
    pub last_seen_at: DateTime<Utc>,
    pub last_seen_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub page: u32,
    pub page_size: u32,
    pub total: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeNodeStats {
    pub filtered_count: u32,
    pub total_count: u32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenThroughputQuery {
    pub window_seconds: Option<u32>,
    pub interval_seconds: Option<u32>,
    pub region: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenThroughputData {
    pub window_seconds: u32,
    pub interval_seconds: u32,
    pub latest: TokenThroughputPoint,
    pub peaks: TokenThroughputPeaks,
    pub totals: TokenThroughputTotals,
    pub points: Vec<TokenThroughputPoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenThroughputPoint {
    pub timestamp: DateTime<Utc>,
    pub input: f64,
    pub output: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct TokenThroughputPeaks {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenThroughputTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

pub async fn get_overview(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<BankingAdminEnvelope<OverviewData>>, StatusCode> {
    let data = banking_admin::get_overview(&app_state.db_pool, &query)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get banking admin overview: {}", e);
            if e.to_string().contains("invalid overview") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    Ok(Json(BankingAdminEnvelope::ok(data)))
}

pub async fn get_network_map(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<NetworkMapQuery>,
) -> Result<Json<BankingAdminEnvelope<NetworkMapData>>, StatusCode> {
    let data = banking_admin::get_network_map(&app_state.db_pool, &query)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get banking admin network map: {}", e);
            if e.to_string().contains("invalid node status") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    Ok(Json(BankingAdminEnvelope::ok(data)))
}

pub async fn get_compute_nodes(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<ComputeNodesQuery>,
) -> Result<Json<BankingAdminEnvelope<ComputeNodesData>>, StatusCode> {
    let data = banking_admin::get_compute_nodes(&app_state.db_pool, &query)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get banking admin compute nodes: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(BankingAdminEnvelope::ok(data)))
}

pub async fn get_token_throughput(
    State(app_state): State<Arc<ApiServer>>,
    Query(query): Query<TokenThroughputQuery>,
) -> Result<Json<BankingAdminEnvelope<TokenThroughputData>>, StatusCode> {
    let data = banking_admin::get_token_throughput(&app_state.db_pool, &query)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get banking admin token throughput: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(BankingAdminEnvelope::ok(data)))
}
