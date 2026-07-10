use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::{postgres::Postgres, FromRow, Pool};
use std::{collections::HashMap, fmt::Write};

use crate::api_server::banking_admin::{
    ClusterStackItem, ComputeNodeItem, ComputeNodeStats, ComputeNodesData, ComputeNodesQuery,
    HighlightProvince, NetworkCity, NetworkLink, NetworkMapData, NetworkMapQuery, NetworkRegion,
    OverviewData, OverviewQuery, OverviewStatusBreakdown, OverviewStatusMetrics, Pagination,
    ResourceUsage, SummaryCard, TokenThroughputData, TokenThroughputPeaks, TokenThroughputPoint,
    TokenThroughputQuery, TokenThroughputTotals, TopCity,
};
use crate::db::token_usage;

#[derive(Debug, FromRow)]
struct BankingAssetRow {
    client_id: String,
    user_id: Option<String>,
    client_name: Option<String>,
    client_status: Option<String>,
    valid_status: Option<String>,
    os_type: Option<String>,
    geo_country: Option<String>,
    geo_region: Option<String>,
    geo_city: Option<String>,
    geo_latitude: Option<f64>,
    geo_longitude: Option<f64>,
    total_tflops: Option<i32>,
    model: Option<String>,
    model_version: Option<String>,
    cpu_usage: Option<i16>,
    mem_usage: Option<i16>,
    disk_usage: Option<i16>,
    gpu_count: i64,
    device_names: Option<Vec<String>>,
    avg_device_gpuusage: Option<f64>,
    token_tps: Option<f64>,
    last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeStatusFilter {
    All,
    Online,
    Offline,
}

#[derive(Debug, Clone)]
struct CityMeta {
    id: String,
    name: String,
    province: String,
    region_id: String,
    region_name: String,
    lng: Option<f64>,
    lat: Option<f64>,
}

#[derive(Debug)]
struct CityAggregate {
    meta: CityMeta,
    lng_sum: f64,
    lat_sum: f64,
    coord_count: u32,
    nodes: u32,
    online_nodes: u32,
    used_nodes: u32,
    tflops: u32,
    gpu_counts: HashMap<String, u32>,
}

impl CityAggregate {
    fn new(meta: CityMeta) -> Self {
        Self {
            meta,
            lng_sum: 0.0,
            lat_sum: 0.0,
            coord_count: 0,
            nodes: 0,
            online_nodes: 0,
            used_nodes: 0,
            tflops: 0,
            gpu_counts: HashMap::new(),
        }
    }

    fn add_row(&mut self, row: &BankingAssetRow) {
        self.nodes = self.nodes.saturating_add(1);
        self.tflops = self
            .tflops
            .saturating_add(row.total_tflops.unwrap_or_default().max(0) as u32);

        let status =
            normalize_compute_status(row.client_status.as_deref(), row.valid_status.as_deref());
        if matches!(status.as_str(), "active" | "online") {
            self.online_nodes = self.online_nodes.saturating_add(1);
        }
        if is_used_node(row) {
            self.used_nodes = self.used_nodes.saturating_add(1);
        }

        let lng = self.meta.lng.or(row.geo_longitude);
        let lat = self.meta.lat.or(row.geo_latitude);
        if let (Some(lng), Some(lat)) = (lng, lat) {
            self.lng_sum += lng;
            self.lat_sum += lat;
            self.coord_count = self.coord_count.saturating_add(1);
        }

        for name in normalized_device_names(row) {
            *self.gpu_counts.entry(name).or_insert(0) += 1;
        }
    }

    fn into_city(self) -> Option<NetworkCity> {
        if self.nodes == 0 || self.coord_count == 0 {
            return None;
        }

        Some(NetworkCity {
            id: self.meta.id,
            name: self.meta.name,
            province: self.meta.province,
            coord: [
                round4(self.lng_sum / self.coord_count as f64),
                round4(self.lat_sum / self.coord_count as f64),
            ],
            nodes: self.nodes,
            tflops: self.tflops,
            gpu_model: top_gpu_models(&self.gpu_counts).unwrap_or_else(|| "Unknown".to_string()),
            tier: city_tier(self.nodes, self.tflops).to_string(),
            online_nodes: self.online_nodes,
            used_nodes: self.used_nodes,
        })
    }
}

#[derive(Debug)]
struct OverviewInventoryMetrics {
    nodes: u32,
    compute_tflops: u64,
    used_devices: u32,
    usage_rate: u8,
    cluster_stack: Vec<ClusterStackItem>,
}

impl OverviewInventoryMetrics {
    fn resource_usage(&self) -> ResourceUsage {
        ResourceUsage {
            total_devices: self.nodes,
            used_devices: self.used_devices,
            usage_rate: self.usage_rate,
        }
    }

    fn status_metrics(&self) -> OverviewStatusMetrics {
        OverviewStatusMetrics {
            nodes: self.nodes,
            compute: self.compute_tflops,
            compute_display_value: format_pf(self.compute_tflops),
            compute_unit: "PF".to_string(),
            resource_usage: self.resource_usage(),
            cluster_stack: self.cluster_stack.clone(),
        }
    }
}

fn overview_rows_by_status<'a>(
    rows: &[&'a BankingAssetRow],
    filter: NodeStatusFilter,
) -> Vec<&'a BankingAssetRow> {
    rows.iter()
        .copied()
        .filter(|row| row_matches_node_status(row, filter))
        .collect()
}

fn overview_inventory_metrics(rows: &[&BankingAssetRow]) -> OverviewInventoryMetrics {
    let nodes = rows.len() as u32;
    let compute_tflops = rows
        .iter()
        .map(|row| row.total_tflops.unwrap_or_default().max(0) as u64)
        .sum();
    let used_devices = rows.iter().filter(|row| is_used_node(row)).count() as u32;

    OverviewInventoryMetrics {
        nodes,
        compute_tflops,
        used_devices,
        usage_rate: percent(used_devices, nodes),
        cluster_stack: cluster_stack(rows),
    }
}

fn node_summary_card(key: &str, label: &str, nodes: u32) -> SummaryCard {
    SummaryCard {
        key: key.to_string(),
        label: label.to_string(),
        value: json!(nodes),
        display_value: format_number(nodes as u64),
        unit: "个".to_string(),
        caption: None,
    }
}

fn compute_summary_card(key: &str, label: &str, compute_tflops: u64) -> SummaryCard {
    SummaryCard {
        key: key.to_string(),
        label: label.to_string(),
        value: json!(compute_tflops),
        display_value: format_pf(compute_tflops),
        unit: "PF".to_string(),
        caption: None,
    }
}

pub async fn get_overview(pool: &Pool<Postgres>, query: &OverviewQuery) -> Result<OverviewData> {
    let time_range = overview_time_range(query)?;
    let rows = fetch_asset_rows(pool).await?;
    let filtered_rows: Vec<&BankingAssetRow> = rows
        .iter()
        .filter(|row| country_allowed(row.geo_country.as_deref()))
        .filter(|row| row_matches_region(row, query.region.as_deref()))
        .collect();

    let online_rows = overview_rows_by_status(&filtered_rows, NodeStatusFilter::Online);
    let offline_rows = overview_rows_by_status(&filtered_rows, NodeStatusFilter::Offline);
    let all_metrics = overview_inventory_metrics(&filtered_rows);
    let online_metrics = overview_inventory_metrics(&online_rows);
    let offline_metrics = overview_inventory_metrics(&offline_rows);
    let token_total = match time_range {
        Some((from, to)) => {
            token_usage::get_token_usage_total_in_range(pool, from, to, query.region.as_deref())
                .await?
        }
        None => token_usage::get_token_usage_summary_today(pool, query.region.as_deref()).await?,
    };
    let token_latest = token_usage::get_token_usage_latest_window(
        pool,
        token_usage::REALTIME_TPS_WINDOW_SECONDS,
        query.region.as_deref(),
    )
    .await?;
    let token_tps = token_usage::tokens_per_second(
        token_latest.total_tokens,
        token_usage::REALTIME_TPS_WINDOW_SECONDS,
    );
    let total_token_value = token_total.total_tokens.max(0) as u64;
    let token_display = format_token_total(total_token_value);

    Ok(OverviewData {
        summary_cards: vec![
            node_summary_card("onlineNodes", "在线节点", online_metrics.nodes),
            compute_summary_card("totalCompute", "总算力", all_metrics.compute_tflops),
            node_summary_card("offlineNodes", "离线节点", offline_metrics.nodes),
            node_summary_card("allNodes", "全部节点", all_metrics.nodes),
            compute_summary_card("onlineCompute", "在线算力", online_metrics.compute_tflops),
            compute_summary_card("offlineCompute", "离线算力", offline_metrics.compute_tflops),
            compute_summary_card("allCompute", "全部算力", all_metrics.compute_tflops),
            SummaryCard {
                key: "realtimeTokenThroughput".to_string(),
                label: "实时Token吞吐".to_string(),
                value: json!(token_tps),
                display_value: format_rate(token_tps),
                unit: "/s".to_string(),
                caption: Some("当前TPS".to_string()),
            },
            SummaryCard {
                key: "todayTokenTotal".to_string(),
                label: "今日Token总量".to_string(),
                value: json!(total_token_value),
                display_value: token_display.0,
                unit: token_display.1,
                caption: Some(if time_range.is_some() {
                    "所选时段调用".to_string()
                } else {
                    "今日已调用".to_string()
                }),
            },
        ],
        resource_usage: all_metrics.resource_usage(),
        cluster_stack: all_metrics.cluster_stack.clone(),
        status_breakdown: OverviewStatusBreakdown {
            all: all_metrics.status_metrics(),
            online: online_metrics.status_metrics(),
            offline: offline_metrics.status_metrics(),
        },
    })
}

pub async fn get_network_map(
    pool: &Pool<Postgres>,
    query: &NetworkMapQuery,
) -> Result<NetworkMapData> {
    let node_status =
        parse_node_status_filter(query.node_status.as_deref(), query.status.as_deref())?;
    let rows = fetch_asset_rows(pool).await?;
    let mut cities: HashMap<String, CityAggregate> = HashMap::new();

    for row in rows
        .iter()
        .filter(|row| country_allowed(row.geo_country.as_deref()))
        .filter(|row| row_matches_node_status(row, node_status))
    {
        let Some(raw_city) = row.geo_city.as_deref() else {
            continue;
        };
        let meta = city_meta(raw_city, row.geo_region.as_deref());
        if meta.lng.or(row.geo_longitude).is_none() || meta.lat.or(row.geo_latitude).is_none() {
            continue;
        }
        cities
            .entry(meta.id.clone())
            .or_insert_with(|| CityAggregate::new(meta))
            .add_row(row);
    }

    let mut cities: Vec<NetworkCity> = cities
        .into_values()
        .filter_map(CityAggregate::into_city)
        .collect();
    cities.sort_by(|a, b| {
        b.tflops
            .cmp(&a.tflops)
            .then_with(|| b.nodes.cmp(&a.nodes))
            .then_with(|| a.id.cmp(&b.id))
    });

    let regions = build_regions(&cities);
    let highlight_provinces = build_highlight_provinces(&cities);
    let top_cities = cities
        .iter()
        .take(5)
        .map(|city| TopCity {
            city_id: city.id.clone(),
            name: city.name.clone(),
            nodes: city.nodes,
            tflops: city.tflops,
        })
        .collect();

    Ok(NetworkMapData {
        cities,
        links: Vec::<NetworkLink>::new(),
        regions,
        highlight_provinces,
        top_cities,
    })
}

pub async fn get_compute_nodes(
    pool: &Pool<Postgres>,
    query: &ComputeNodesQuery,
) -> Result<ComputeNodesData> {
    let rows = fetch_asset_rows(pool).await?;
    let total_count = rows
        .iter()
        .filter(|row| country_allowed(row.geo_country.as_deref()))
        .count() as u32;

    let mut items: Vec<ComputeNodeItem> = rows
        .iter()
        .filter(|row| country_allowed(row.geo_country.as_deref()))
        .filter_map(asset_row_to_compute_node)
        .filter(|item| compute_node_matches_query(item, query))
        .collect();

    items.sort_by(|a, b| {
        b.last_seen_at
            .cmp(&a.last_seen_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    let filtered_count = items.len() as u32;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 200);
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let paged_items = items
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect::<Vec<_>>();

    Ok(ComputeNodesData {
        items: paged_items,
        pagination: Pagination {
            page,
            page_size,
            total: filtered_count,
        },
        stats: ComputeNodeStats {
            filtered_count,
            total_count,
        },
    })
}

pub async fn get_token_throughput(
    pool: &Pool<Postgres>,
    query: &TokenThroughputQuery,
) -> Result<TokenThroughputData> {
    const MAX_THROUGHPUT_POINTS: u32 = 600;

    let window_seconds = query.window_seconds.unwrap_or(180).clamp(1, 86_400);
    let requested_interval_seconds = query.interval_seconds.unwrap_or(3).clamp(1, 300);
    let min_interval_seconds =
        window_seconds.saturating_add(MAX_THROUGHPUT_POINTS - 2) / (MAX_THROUGHPUT_POINTS - 1);
    let interval_seconds = requested_interval_seconds.max(min_interval_seconds.max(1));
    let point_count = (window_seconds / interval_seconds)
        .saturating_add(1)
        .clamp(1, MAX_THROUGHPUT_POINTS);
    let now = Utc::now();
    let rows = token_usage::get_token_usage_points(
        pool,
        window_seconds,
        interval_seconds,
        query.region.as_deref(),
    )
    .await?;
    let total_row = token_usage::get_token_usage_io_latest_window(
        pool,
        window_seconds,
        query.region.as_deref(),
    )
    .await?;

    let mut points = Vec::with_capacity(point_count as usize);
    if rows.is_empty() {
        for index in (0..point_count).rev() {
            points.push(TokenThroughputPoint {
                timestamp: now - Duration::seconds((index * interval_seconds) as i64),
                input: 0.0,
                output: 0.0,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
            });
        }
    } else {
        let skip_count = rows.len().saturating_sub(point_count as usize);
        for row in rows.into_iter().skip(skip_count) {
            let input_tokens = row.input_tokens.max(0) as u64;
            let output_tokens = row.output_tokens.max(0) as u64;
            points.push(TokenThroughputPoint {
                timestamp: row.bucket,
                input: token_usage::tokens_per_second(row.input_tokens, interval_seconds),
                output: token_usage::tokens_per_second(row.output_tokens, interval_seconds),
                input_tokens,
                output_tokens,
                total_tokens: input_tokens.saturating_add(output_tokens),
            });
        }
    }

    let totals = TokenThroughputTotals {
        input_tokens: total_row.input_tokens.max(0) as u64,
        output_tokens: total_row.output_tokens.max(0) as u64,
        total_tokens: total_row.total_tokens.max(0) as u64,
    };

    if points.is_empty() {
        points.push(TokenThroughputPoint {
            timestamp: now,
            input: 0.0,
            output: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        });
    }

    let latest = points.last().cloned().unwrap_or(TokenThroughputPoint {
        timestamp: now,
        input: 0.0,
        output: 0.0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
    });

    Ok(TokenThroughputData {
        window_seconds,
        interval_seconds,
        latest,
        peaks: TokenThroughputPeaks {
            input: points.iter().fold(0.0, |peak, point| peak.max(point.input)),
            output: points
                .iter()
                .fold(0.0, |peak, point| peak.max(point.output)),
        },
        totals,
        points,
    })
}

async fn fetch_asset_rows(pool: &Pool<Postgres>) -> Result<Vec<BankingAssetRow>> {
    let rows = sqlx::query_as::<_, BankingAssetRow>(
        r#"
        SELECT
            ENCODE(ga.client_id, 'hex') AS client_id,
            ga.user_id,
            ga.client_name,
            ga.client_status,
            ga.valid_status,
            ga.os_type,
            ga.geo_country,
            ga.geo_region,
            ga.geo_city,
            ga.geo_latitude,
            ga.geo_longitude,
            si.total_tflops,
            ga.model,
            ga.model_version,
            si.cpu_usage,
            si.mem_usage,
            si.disk_usage,
            COALESCE(di.gpu_count, 0)::BIGINT AS gpu_count,
            COALESCE(di.device_names, ARRAY[]::TEXT[]) AS device_names,
            di.avg_device_gpuusage,
            COALESCE(tu.token_tps, 0)::FLOAT8 AS token_tps,
            GREATEST(
                COALESCE(ga.updated_at, NOW())::TIMESTAMPTZ,
                COALESCE(si.updated_at, ga.updated_at, NOW())::TIMESTAMPTZ,
                COALESCE(di.last_device_updated_at, ga.updated_at, NOW())::TIMESTAMPTZ
            ) AS last_seen_at
        FROM gpu_assets ga
        LEFT JOIN system_info si ON si.client_id = ga.client_id
        LEFT JOIN (
            SELECT
                client_id,
                COUNT(*)::BIGINT AS gpu_count,
                ARRAY_AGG(DISTINCT device_name) FILTER (WHERE device_name IS NOT NULL AND device_name <> '') AS device_names,
                AVG(device_gpuusage)::FLOAT8 AS avg_device_gpuusage,
                MAX(updated_at)::TIMESTAMPTZ AS last_device_updated_at
            FROM device_info
            GROUP BY client_id
        ) di ON di.client_id = ga.client_id
        LEFT JOIN (
            SELECT client_id, SUM(total_tokens)::FLOAT8 / 10.0 AS token_tps
            FROM inference_token_usage
            WHERE success = TRUE
              AND created_at >= NOW() - INTERVAL '10 seconds'
            GROUP BY client_id
        ) tu ON tu.client_id = ga.client_id
        WHERE COALESCE(ga.valid_status, 'valid') IN ('valid', 'warning')
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

fn asset_row_to_compute_node(row: &BankingAssetRow) -> Option<ComputeNodeItem> {
    let meta = row
        .geo_city
        .as_deref()
        .map(|city| city_meta(city, row.geo_region.as_deref()));
    let region_name = meta
        .as_ref()
        .map(|meta| meta.name.clone())
        .or_else(|| row.geo_region.clone())
        .unwrap_or_else(|| "未知地区".to_string());
    let region_id = meta.as_ref().map(|meta| meta.id.clone());
    let owner = row
        .user_id
        .as_deref()
        .filter(|user_id| !user_id.trim().is_empty())
        .map(|user_id| format!("用户 {}", user_id.trim()))
        .or_else(|| {
            meta.as_ref()
                .map(|meta| format!("{}节点池", meta.region_name))
        })
        .unwrap_or_else(|| "默认节点池".to_string());
    let gpu_model = top_gpu_models_from_row(row);
    let gpu_count = (row.gpu_count > 0).then_some(row.gpu_count as u32);
    let gpu = format_gpu(gpu_count, gpu_model.as_deref());
    let name = row
        .client_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", region_name, short_id(&row.client_id)));

    Some(ComputeNodeItem {
        id: row.client_id.clone(),
        name,
        owner,
        region: region_name,
        region_id,
        device: normalize_device(row.os_type.as_deref()),
        status: normalize_compute_status(row.client_status.as_deref(), row.valid_status.as_deref()),
        gpu,
        gpu_model,
        gpu_count,
        load: current_load(row),
        tokens_per_second: row.token_tps.unwrap_or_default().max(0.0),
        last_seen_at: row.last_seen_at,
        last_seen_text: Some(last_seen_text(row.last_seen_at)),
    })
}

fn compute_node_matches_query(item: &ComputeNodeItem, query: &ComputeNodesQuery) -> bool {
    if let Some(status) = query.status.as_deref().map(normalize_filter) {
        if normalize_filter(&item.status) != status {
            return false;
        }
    }

    if let Some(device) = query.device.as_deref().map(normalize_filter) {
        if normalize_filter(&item.device) != device {
            return false;
        }
    }

    if let Some(region) = query.region.as_deref().map(normalize_filter) {
        let region_id = item.region_id.as_deref().map(normalize_filter);
        let matches_region = region_id.as_deref() == Some(region.as_str())
            || normalize_filter(&item.region) == region
            || normalize_filter(&item.owner).contains(&region);
        if !matches_region {
            return false;
        }
    }

    if let Some(keyword) = query.keyword.as_deref() {
        let keyword = normalize_filter(keyword);
        if !keyword.is_empty() {
            let haystack = [
                item.id.as_str(),
                item.name.as_str(),
                item.owner.as_str(),
                item.region.as_str(),
                item.gpu.as_str(),
                item.gpu_model.as_deref().unwrap_or_default(),
            ]
            .join(" ");
            if !normalize_filter(&haystack).contains(&keyword) {
                return false;
            }
        }
    }

    true
}

fn row_matches_region(row: &BankingAssetRow, region: Option<&str>) -> bool {
    let Some(region) = region
        .map(normalize_filter)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };

    let meta = row
        .geo_city
        .as_deref()
        .map(|city| city_meta(city, row.geo_region.as_deref()));
    if let Some(meta) = meta {
        return normalize_filter(&meta.id) == region
            || normalize_filter(&meta.name) == region
            || normalize_filter(&meta.province) == region
            || normalize_filter(&meta.region_id) == region
            || normalize_filter(&meta.region_name) == region;
    }

    row.geo_region
        .as_deref()
        .map(|raw| normalize_filter(raw) == region)
        .unwrap_or(false)
}

fn cluster_stack(rows: &[&BankingAssetRow]) -> Vec<ClusterStackItem> {
    let mut a100_h100 = 0_u32;
    let mut a800_4090 = 0_u32;
    let mut cpu_edge = 0_u32;

    for row in rows {
        match classify_stack(row) {
            "a100_h100" => a100_h100 = a100_h100.saturating_add(1),
            "a800_4090" => a800_4090 = a800_4090.saturating_add(1),
            _ => cpu_edge = cpu_edge.saturating_add(1),
        }
    }

    let total = rows.len() as u32;
    let first = percent(a100_h100, total);
    let second = percent(a800_4090, total);
    let third = if total == 0 {
        0
    } else {
        100_u8.saturating_sub(first).saturating_sub(second)
    };

    vec![
        ClusterStackItem {
            key: "a100_h100".to_string(),
            label: "GPU A100/H100".to_string(),
            percent: first,
        },
        ClusterStackItem {
            key: "a800_4090".to_string(),
            label: "GPU A800/4090".to_string(),
            percent: second,
        },
        ClusterStackItem {
            key: "cpu_edge".to_string(),
            label: "CPU/边缘节点".to_string(),
            percent: third,
        },
    ]
}

fn classify_stack(row: &BankingAssetRow) -> &'static str {
    let names = normalized_device_names(row).join(" ").to_ascii_lowercase();
    if names.contains("h100") || names.contains("a100") {
        "a100_h100"
    } else if names.contains("a800") || names.contains("4090") {
        "a800_4090"
    } else {
        "cpu_edge"
    }
}

fn build_regions(cities: &[NetworkCity]) -> Vec<NetworkRegion> {
    let mut city_ids_by_region: HashMap<String, Vec<String>> = HashMap::new();
    for city in cities {
        let meta = city_meta(&city.name, None);
        city_ids_by_region
            .entry(meta.region_id)
            .or_default()
            .push(city.id.clone());
    }

    region_defs()
        .into_iter()
        .map(|(id, name)| {
            let mut city_ids = city_ids_by_region.remove(id).unwrap_or_default();
            city_ids.sort();
            NetworkRegion {
                id: id.to_string(),
                name: name.to_string(),
                active: !city_ids.is_empty(),
                city_ids,
            }
        })
        .collect()
}

fn build_highlight_provinces(cities: &[NetworkCity]) -> Vec<HighlightProvince> {
    let mut province_scores: HashMap<String, (u32, u32)> = HashMap::new();
    for city in cities {
        let entry = province_scores
            .entry(city.province.clone())
            .or_insert((0, 0));
        entry.0 = entry.0.saturating_add(city.tflops);
        entry.1 = entry.1.saturating_add(city.nodes);
    }

    let mut provinces: Vec<(String, u32, u32)> = province_scores
        .into_iter()
        .map(|(province, (tflops, nodes))| (province, tflops, nodes))
        .collect();
    provinces.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });

    provinces
        .into_iter()
        .take(8)
        .enumerate()
        .map(|(index, (name, _tflops, _nodes))| HighlightProvince {
            name,
            level: if index < 3 { "high" } else { "medium" }.to_string(),
        })
        .collect()
}

fn city_meta(raw_city: &str, raw_region: Option<&str>) -> CityMeta {
    let normalized = normalize_lookup_key(raw_city);
    match normalized.as_str() {
        "beijing" | "北京" | "北京市" => known_city(
            "beijing",
            "北京",
            "北京市",
            "north",
            "华北算力区",
            116.4074,
            39.9042,
        ),
        "shanghai" | "上海" | "上海市" => known_city(
            "shanghai",
            "上海",
            "上海市",
            "east",
            "华东算力区",
            121.4737,
            31.2304,
        ),
        "shenzhen" | "深圳" | "深圳市" => known_city(
            "shenzhen",
            "深圳",
            "广东省",
            "south",
            "华南算力区",
            114.0579,
            22.5431,
        ),
        "guangzhou" | "广州" | "广州市" => known_city(
            "guangzhou",
            "广州",
            "广东省",
            "south",
            "华南算力区",
            113.2644,
            23.1291,
        ),
        "hangzhou" | "杭州" | "杭州市" => known_city(
            "hangzhou",
            "杭州",
            "浙江省",
            "east",
            "华东算力区",
            120.1551,
            30.2741,
        ),
        "nanjing" | "南京" | "南京市" => known_city(
            "nanjing",
            "南京",
            "江苏省",
            "east",
            "华东算力区",
            118.7969,
            32.0603,
        ),
        "chengdu" | "成都" | "成都市" => known_city(
            "chengdu",
            "成都",
            "四川省",
            "southwest",
            "西南算力区",
            104.0665,
            30.5728,
        ),
        "chongqing" | "重庆" | "重庆市" => known_city(
            "chongqing",
            "重庆",
            "重庆市",
            "southwest",
            "西南算力区",
            106.5516,
            29.563,
        ),
        "wuhan" | "武汉" | "武汉市" => known_city(
            "wuhan",
            "武汉",
            "湖北省",
            "central",
            "华中算力区",
            114.3054,
            30.5931,
        ),
        "xian" | "xi'an" | "西安" | "西安市" => known_city(
            "xian",
            "西安",
            "陕西省",
            "northwest",
            "西北算力区",
            108.9398,
            34.3416,
        ),
        "tianjin" | "天津" | "天津市" => known_city(
            "tianjin",
            "天津",
            "天津市",
            "north",
            "华北算力区",
            117.2009,
            39.0842,
        ),
        "hohhot" | "呼和浩特" | "呼和浩特市" => known_city(
            "hohhot",
            "呼和浩特",
            "内蒙古自治区",
            "north",
            "华北算力区",
            111.7492,
            40.8426,
        ),
        "qingdao" | "青岛" | "青岛市" => known_city(
            "qingdao",
            "青岛",
            "山东省",
            "east",
            "华东算力区",
            120.3826,
            36.0671,
        ),
        "suzhou" | "苏州" | "苏州市" => known_city(
            "suzhou",
            "苏州",
            "江苏省",
            "east",
            "华东算力区",
            120.5853,
            31.2989,
        ),
        "changsha" | "长沙" | "长沙市" => known_city(
            "changsha",
            "长沙",
            "湖南省",
            "central",
            "华中算力区",
            112.9388,
            28.2282,
        ),
        "zhengzhou" | "郑州" | "郑州市" => known_city(
            "zhengzhou",
            "郑州",
            "河南省",
            "central",
            "华中算力区",
            113.6254,
            34.7466,
        ),
        "jinan" | "济南" | "济南市" => known_city(
            "jinan",
            "济南",
            "山东省",
            "east",
            "华东算力区",
            117.1201,
            36.6512,
        ),
        "shenyang" | "沈阳" | "沈阳市" => known_city(
            "shenyang",
            "沈阳",
            "辽宁省",
            "northeast",
            "东北算力区",
            123.4315,
            41.8057,
        ),
        "dalian" | "大连" | "大连市" => known_city(
            "dalian",
            "大连",
            "辽宁省",
            "northeast",
            "东北算力区",
            121.6147,
            38.914,
        ),
        "harbin" | "哈尔滨" | "哈尔滨市" => known_city(
            "harbin",
            "哈尔滨",
            "黑龙江省",
            "northeast",
            "东北算力区",
            126.6425,
            45.7567,
        ),
        "kunming" | "昆明" | "昆明市" => known_city(
            "kunming",
            "昆明",
            "云南省",
            "southwest",
            "西南算力区",
            102.8329,
            24.8801,
        ),
        "guiyang" | "贵阳" | "贵阳市" => known_city(
            "guiyang",
            "贵阳",
            "贵州省",
            "southwest",
            "西南算力区",
            106.6302,
            26.6477,
        ),
        "lanzhou" | "兰州" | "兰州市" => known_city(
            "lanzhou",
            "兰州",
            "甘肃省",
            "northwest",
            "西北算力区",
            103.8343,
            36.0611,
        ),
        "urumqi" | "乌鲁木齐" | "乌鲁木齐市" => known_city(
            "urumqi",
            "乌鲁木齐",
            "新疆维吾尔自治区",
            "northwest",
            "西北算力区",
            87.6168,
            43.8256,
        ),
        "hongkong" | "hong kong" | "香港" | "香港特别行政区" => known_city(
            "hongkong",
            "香港",
            "香港特别行政区",
            "south",
            "华南算力区",
            114.1694,
            22.3193,
        ),
        _ => {
            let name = raw_city.trim();
            let (region_id, region_name) = region_from_geo(raw_region);
            CityMeta {
                id: slugify_city(name),
                name: if name.is_empty() {
                    "未知城市".to_string()
                } else {
                    name.to_string()
                },
                province: province_from_geo(raw_region).unwrap_or_else(|| "未知地区".to_string()),
                region_id: region_id.to_string(),
                region_name: region_name.to_string(),
                lng: None,
                lat: None,
            }
        }
    }
}

fn known_city(
    id: &str,
    name: &str,
    province: &str,
    region_id: &str,
    region_name: &str,
    lng: f64,
    lat: f64,
) -> CityMeta {
    CityMeta {
        id: id.to_string(),
        name: name.to_string(),
        province: province.to_string(),
        region_id: region_id.to_string(),
        region_name: region_name.to_string(),
        lng: Some(lng),
        lat: Some(lat),
    }
}

fn region_defs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("north", "华北算力区"),
        ("east", "华东算力区"),
        ("south", "华南算力区"),
        ("central", "华中算力区"),
        ("southwest", "西南算力区"),
        ("northwest", "西北算力区"),
        ("northeast", "东北算力区"),
        ("unknown", "未知算力区"),
    ]
}

fn region_from_geo(raw_region: Option<&str>) -> (&'static str, &'static str) {
    match raw_region.map(normalize_lookup_key).as_deref() {
        Some("beijing")
        | Some("tianjin")
        | Some("hebei")
        | Some("shanxi")
        | Some("innermongolia")
        | Some("内蒙古")
        | Some("河北")
        | Some("山西")
        | Some("北京")
        | Some("天津") => ("north", "华北算力区"),
        Some("shanghai") | Some("jiangsu") | Some("zhejiang") | Some("anhui") | Some("fujian")
        | Some("jiangxi") | Some("shandong") | Some("上海") | Some("江苏") | Some("浙江")
        | Some("安徽") | Some("福建") | Some("江西") | Some("山东") => {
            ("east", "华东算力区")
        }
        Some("guangdong") | Some("guangxi") | Some("hainan") | Some("hongkong") | Some("macau")
        | Some("广东") | Some("广西") | Some("海南") | Some("香港") | Some("澳门") => {
            ("south", "华南算力区")
        }
        Some("henan") | Some("hubei") | Some("hunan") | Some("河南") | Some("湖北")
        | Some("湖南") => ("central", "华中算力区"),
        Some("chongqing") | Some("sichuan") | Some("guizhou") | Some("yunnan") | Some("tibet")
        | Some("重庆") | Some("四川") | Some("贵州") | Some("云南") | Some("西藏") => {
            ("southwest", "西南算力区")
        }
        Some("shaanxi") | Some("gansu") | Some("qinghai") | Some("ningxia") | Some("xinjiang")
        | Some("陕西") | Some("甘肃") | Some("青海") | Some("宁夏") | Some("新疆") => {
            ("northwest", "西北算力区")
        }
        Some("liaoning") | Some("jilin") | Some("heilongjiang") | Some("辽宁") | Some("吉林")
        | Some("黑龙江") => ("northeast", "东北算力区"),
        _ => ("unknown", "未知算力区"),
    }
}

fn province_from_geo(raw_region: Option<&str>) -> Option<String> {
    let province = match raw_region.map(normalize_lookup_key).as_deref()? {
        "beijing" | "北京" => "北京市",
        "tianjin" | "天津" => "天津市",
        "hebei" | "河北" => "河北省",
        "shanxi" | "山西" => "山西省",
        "innermongolia" | "内蒙古" => "内蒙古自治区",
        "shanghai" | "上海" => "上海市",
        "jiangsu" | "江苏" => "江苏省",
        "zhejiang" | "浙江" => "浙江省",
        "anhui" | "安徽" => "安徽省",
        "fujian" | "福建" => "福建省",
        "jiangxi" | "江西" => "江西省",
        "shandong" | "山东" => "山东省",
        "guangdong" | "广东" => "广东省",
        "guangxi" | "广西" => "广西壮族自治区",
        "hainan" | "海南" => "海南省",
        "hongkong" | "香港" => "香港特别行政区",
        "macau" | "澳门" => "澳门特别行政区",
        "henan" | "河南" => "河南省",
        "hubei" | "湖北" => "湖北省",
        "hunan" | "湖南" => "湖南省",
        "chongqing" | "重庆" => "重庆市",
        "sichuan" | "四川" => "四川省",
        "guizhou" | "贵州" => "贵州省",
        "yunnan" | "云南" => "云南省",
        "tibet" | "西藏" => "西藏自治区",
        "shaanxi" | "陕西" => "陕西省",
        "gansu" | "甘肃" => "甘肃省",
        "qinghai" | "青海" => "青海省",
        "ningxia" | "宁夏" => "宁夏回族自治区",
        "xinjiang" | "新疆" => "新疆维吾尔自治区",
        "liaoning" | "辽宁" => "辽宁省",
        "jilin" | "吉林" => "吉林省",
        "heilongjiang" | "黑龙江" => "黑龙江省",
        _ => return None,
    };
    Some(province.to_string())
}

fn country_allowed(country: Option<&str>) -> bool {
    match country.map(normalize_lookup_key).as_deref() {
        None | Some("") => true,
        Some("china") | Some("cn") | Some("中国") | Some("hongkong") | Some("hk")
        | Some("hongkongsar") | Some("macao") | Some("macau") | Some("taiwan") => true,
        _ => false,
    }
}

fn normalize_compute_status(client_status: Option<&str>, valid_status: Option<&str>) -> String {
    let client_status = client_status
        .map(|status| status.trim().to_ascii_lowercase())
        .unwrap_or_default();

    if matches!(client_status.as_str(), "error" | "maintenance") {
        return client_status;
    }

    if matches!(
        valid_status
            .map(|status| status.trim().to_ascii_lowercase())
            .as_deref(),
        Some("warning")
    ) {
        return "warning".to_string();
    }

    match client_status.as_str() {
        "active" | "online" | "warning" | "offline" => client_status,
        _ => "offline".to_string(),
    }
}

fn parse_node_status_filter(
    node_status: Option<&str>,
    status_alias: Option<&str>,
) -> Result<NodeStatusFilter> {
    let raw = node_status
        .or(status_alias)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(raw) = raw else {
        return Ok(NodeStatusFilter::All);
    };

    match normalize_filter(raw).as_str() {
        "all" | "全部" => Ok(NodeStatusFilter::All),
        "online" | "active" | "在线" => Ok(NodeStatusFilter::Online),
        "offline" | "离线" => Ok(NodeStatusFilter::Offline),
        _ => Err(anyhow::anyhow!(
            "invalid node status filter: expected all, online, or offline"
        )),
    }
}

fn row_matches_node_status(row: &BankingAssetRow, filter: NodeStatusFilter) -> bool {
    match filter {
        NodeStatusFilter::All => true,
        NodeStatusFilter::Online => is_online_node(row),
        NodeStatusFilter::Offline => !is_online_node(row),
    }
}

fn is_online_node(row: &BankingAssetRow) -> bool {
    matches!(
        normalize_compute_status(row.client_status.as_deref(), row.valid_status.as_deref())
            .as_str(),
        "active" | "online"
    )
}

fn normalize_device(os_type: Option<&str>) -> String {
    let value = os_type.unwrap_or_default().trim().to_ascii_lowercase();
    if value.contains("windows") || value == "win" {
        "windows".to_string()
    } else if value.contains("mac") || value.contains("darwin") || value.contains("osx") {
        "mac".to_string()
    } else if value.contains("linux")
        || value.contains("ubuntu")
        || value.contains("debian")
        || value.contains("centos")
    {
        "linux".to_string()
    } else {
        "unknown".to_string()
    }
}

fn normalized_device_names(row: &BankingAssetRow) -> Vec<String> {
    row.device_names
        .as_ref()
        .into_iter()
        .flatten()
        .filter_map(|name| {
            let name = normalize_gpu_model_name(name);
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

fn normalize_gpu_model_name(name: &str) -> String {
    let normalized = name
        .trim()
        .replace("NVIDIA ", "")
        .replace("GeForce ", "")
        .replace("Graphics", "")
        .trim()
        .to_string();

    let lower = normalized.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "" | "unknown" | "unknown unknown" | "unknown device"
    ) {
        String::new()
    } else {
        normalized
    }
}

fn top_gpu_models_from_row(row: &BankingAssetRow) -> Option<String> {
    let mut counts = HashMap::new();
    for name in normalized_device_names(row) {
        *counts.entry(name).or_insert(0) += 1;
    }
    top_gpu_models(&counts)
}

fn top_gpu_models(counts: &HashMap<String, u32>) -> Option<String> {
    let mut items: Vec<(&String, &u32)> = counts.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let models: Vec<String> = items
        .into_iter()
        .take(3)
        .map(|(name, _)| name.clone())
        .collect();
    if models.is_empty() {
        None
    } else {
        Some(models.join("/"))
    }
}

fn format_gpu(gpu_count: Option<u32>, gpu_model: Option<&str>) -> String {
    match (gpu_count, gpu_model.filter(|model| !model.is_empty())) {
        (Some(count), Some(model)) => format!("{count} x {model}"),
        (None, Some(model)) => model.to_string(),
        (Some(count), None) => format!("{count} x Unknown"),
        (None, None) => "Unknown".to_string(),
    }
}

fn current_load(row: &BankingAssetRow) -> u8 {
    let mut load = 0_f64;
    for value in [
        row.cpu_usage.map(f64::from),
        row.mem_usage.map(f64::from),
        row.disk_usage.map(f64::from),
        row.avg_device_gpuusage,
    ]
    .into_iter()
    .flatten()
    {
        load = load.max(value);
    }
    load.clamp(0.0, 100.0).round() as u8
}

fn is_used_node(row: &BankingAssetRow) -> bool {
    if !is_online_node(row) {
        return false;
    }

    let has_model = row
        .model
        .as_deref()
        .or(row.model_version.as_deref())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    has_model || current_load(row) >= 5
}

fn city_tier(nodes: u32, tflops: u32) -> &'static str {
    if tflops >= 10_000 || nodes >= 100 {
        "mega"
    } else if tflops >= 3_000 || nodes >= 30 {
        "large"
    } else if tflops >= 1_000 || nodes >= 10 {
        "medium"
    } else {
        "small"
    }
}

fn last_seen_text(last_seen_at: DateTime<Utc>) -> String {
    let seconds = (Utc::now() - last_seen_at).num_seconds().max(0);
    if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3_600 {
        format!("{}分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}小时前", seconds / 3_600)
    } else {
        format!("{}天前", seconds / 86_400)
    }
}

fn format_number(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, ch) in raw.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_rate(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn format_pf(total_tflops: u64) -> String {
    let value = total_tflops as f64 / 1_000.0;
    let text = format!("{value:.1}");
    text.trim_end_matches(".0").to_string()
}

fn format_token_total(total_tokens: u64) -> (String, String) {
    let units = [
        ("T", 1_000_000_000_000_f64),
        ("B", 1_000_000_000_f64),
        ("M", 1_000_000_f64),
        ("K", 1_000_f64),
    ];
    let (unit, divisor) = units
        .into_iter()
        .find(|(_, divisor)| total_tokens as f64 >= *divisor)
        .unwrap_or(("tokens", 1.0));
    let value = total_tokens as f64 / divisor;
    let text = format!("{value:.2}");
    (
        text.trim_end_matches('0').trim_end_matches('.').to_string(),
        unit.to_string(),
    )
}

fn percent(value: u32, total: u32) -> u8 {
    if total == 0 {
        0
    } else {
        (((value as f64 / total as f64) * 100.0).round() as u8).min(100)
    }
}

fn normalize_filter(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '_', '-'], "")
}

fn overview_time_range(query: &OverviewQuery) -> Result<Option<(DateTime<Utc>, DateTime<Utc>)>> {
    match (query.from.as_deref(), query.to.as_deref()) {
        (None, None) => Ok(None),
        (from, to) => {
            let now = Utc::now();
            let from = match from {
                Some(value) if !value.trim().is_empty() => parse_api_datetime(value)
                    .with_context(|| format!("invalid overview from timestamp: {value}"))?,
                _ => now - Duration::days(1),
            };
            let to = match to {
                Some(value) if !value.trim().is_empty() => parse_api_datetime(value)
                    .with_context(|| format!("invalid overview to timestamp: {value}"))?,
                _ => now,
            };
            if from >= to {
                return Err(anyhow::anyhow!(
                    "invalid overview time range: from must be earlier than to"
                ));
            }
            Ok(Some((from, to)))
        }
    }
}

fn parse_api_datetime(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn normalize_lookup_key(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '_'], "")
}

fn slugify_city(value: &str) -> String {
    let slug: String = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if slug.is_empty() {
        let mut encoded = String::new();
        for byte in value.trim().as_bytes() {
            let _ = write!(&mut encoded, "{byte:02x}");
        }
        if encoded.is_empty() {
            "city-unknown".to_string()
        } else {
            format!("city-{encoded}")
        }
    } else {
        slug
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(client_status: Option<&str>, valid_status: Option<&str>) -> BankingAssetRow {
        BankingAssetRow {
            client_id: "client-1".to_string(),
            user_id: None,
            client_name: None,
            client_status: client_status.map(str::to_string),
            valid_status: valid_status.map(str::to_string),
            os_type: None,
            geo_country: None,
            geo_region: None,
            geo_city: None,
            geo_latitude: None,
            geo_longitude: None,
            total_tflops: Some(1),
            model: None,
            model_version: None,
            cpu_usage: None,
            mem_usage: None,
            disk_usage: None,
            gpu_count: 0,
            device_names: None,
            avg_device_gpuusage: None,
            token_tps: None,
            last_seen_at: Utc::now(),
        }
    }

    fn asset_with_tflops(
        client_status: Option<&str>,
        valid_status: Option<&str>,
        total_tflops: i32,
    ) -> BankingAssetRow {
        let mut row = asset(client_status, valid_status);
        row.total_tflops = Some(total_tflops);
        row
    }

    #[test]
    fn parses_node_status_filter_aliases() {
        assert_eq!(
            parse_node_status_filter(None, None).unwrap(),
            NodeStatusFilter::All
        );
        assert_eq!(
            parse_node_status_filter(Some("online"), None).unwrap(),
            NodeStatusFilter::Online
        );
        assert_eq!(
            parse_node_status_filter(None, Some("离线")).unwrap(),
            NodeStatusFilter::Offline
        );
        assert_eq!(
            parse_node_status_filter(Some("全部"), Some("online")).unwrap(),
            NodeStatusFilter::All
        );
        assert!(parse_node_status_filter(Some("busy"), None).is_err());
    }

    #[test]
    fn offline_node_filter_matches_non_online_visible_nodes() {
        let online = asset(Some("online"), Some("valid"));
        let active = asset(Some("active"), Some("valid"));
        let offline = asset(Some("offline"), Some("valid"));
        let warning = asset(Some("online"), Some("warning"));
        let maintenance = asset(Some("maintenance"), Some("valid"));

        assert!(row_matches_node_status(&online, NodeStatusFilter::Online));
        assert!(row_matches_node_status(&active, NodeStatusFilter::Online));
        assert!(!row_matches_node_status(&offline, NodeStatusFilter::Online));
        assert!(!row_matches_node_status(&warning, NodeStatusFilter::Online));

        assert!(!row_matches_node_status(&online, NodeStatusFilter::Offline));
        assert!(row_matches_node_status(&offline, NodeStatusFilter::Offline));
        assert!(row_matches_node_status(&warning, NodeStatusFilter::Offline));
        assert!(row_matches_node_status(
            &maintenance,
            NodeStatusFilter::Offline
        ));
    }

    #[test]
    fn overview_inventory_metrics_groups_all_online_offline() {
        let online = asset_with_tflops(Some("online"), Some("valid"), 10);
        let active = asset_with_tflops(Some("active"), Some("valid"), 20);
        let offline = asset_with_tflops(Some("offline"), Some("valid"), 30);
        let warning = asset_with_tflops(Some("online"), Some("warning"), 40);
        let rows = vec![&online, &active, &offline, &warning];

        let online_rows = overview_rows_by_status(&rows, NodeStatusFilter::Online);
        let offline_rows = overview_rows_by_status(&rows, NodeStatusFilter::Offline);
        let all_metrics = overview_inventory_metrics(&rows);
        let online_metrics = overview_inventory_metrics(&online_rows);
        let offline_metrics = overview_inventory_metrics(&offline_rows);

        assert_eq!(all_metrics.nodes, 4);
        assert_eq!(all_metrics.compute_tflops, 100);
        assert_eq!(online_metrics.nodes, 2);
        assert_eq!(online_metrics.compute_tflops, 30);
        assert_eq!(offline_metrics.nodes, 2);
        assert_eq!(offline_metrics.compute_tflops, 70);
        assert_eq!(
            online_metrics.nodes + offline_metrics.nodes,
            all_metrics.nodes
        );
        assert_eq!(
            online_metrics.compute_tflops + offline_metrics.compute_tflops,
            all_metrics.compute_tflops
        );
    }
}
