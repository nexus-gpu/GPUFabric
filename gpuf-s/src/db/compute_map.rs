use anyhow::Result;
use sqlx::{postgres::Postgres, FromRow, Pool};
use std::{collections::HashMap, fmt::Write};

use crate::api_server::compute_map::{
    ComputeMapLink, ComputeMapNode, ComputeMapResponse, ComputeMapSummary,
};
use crate::db::token_usage;

#[derive(Debug, FromRow)]
struct ComputeMapAssetRow {
    client_status: Option<String>,
    valid_status: Option<String>,
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
    avg_device_gpuusage: Option<f64>,
    device_names: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CityStatus {
    Online,
    Warning,
    Offline,
    Maintenance,
}

impl CityStatus {
    fn as_str(self) -> &'static str {
        match self {
            CityStatus::Online => "online",
            CityStatus::Warning => "warning",
            CityStatus::Offline => "offline",
            CityStatus::Maintenance => "maintenance",
        }
    }
}

#[derive(Debug)]
struct CityAggregate {
    id: String,
    name: String,
    lng_sum: f64,
    lat_sum: f64,
    coord_count: u32,
    node_count: u32,
    online_count: u32,
    warning_count: u32,
    maintenance_count: u32,
    used_count: u32,
    tflops: u32,
    gpu_counts: HashMap<String, u32>,
    region: Option<String>,
}

impl CityAggregate {
    fn new(id: String, name: String, region: Option<String>) -> Self {
        Self {
            id,
            name,
            lng_sum: 0.0,
            lat_sum: 0.0,
            coord_count: 0,
            node_count: 0,
            online_count: 0,
            warning_count: 0,
            maintenance_count: 0,
            used_count: 0,
            tflops: 0,
            gpu_counts: HashMap::new(),
            region,
        }
    }

    fn add_row(&mut self, row: ComputeMapAssetRow, meta: CityMeta) {
        self.node_count = self.node_count.saturating_add(1);
        self.tflops = self
            .tflops
            .saturating_add(row.total_tflops.unwrap_or_default().max(0) as u32);

        let status = normalize_status(row.client_status.as_deref(), row.valid_status.as_deref());
        match status {
            CityStatus::Online => self.online_count = self.online_count.saturating_add(1),
            CityStatus::Warning => self.warning_count = self.warning_count.saturating_add(1),
            CityStatus::Maintenance => {
                self.maintenance_count = self.maintenance_count.saturating_add(1)
            }
            CityStatus::Offline => {}
        }
        if is_used_node(&row) {
            self.used_count = self.used_count.saturating_add(1);
        }

        let lng = meta.lng.or(row.geo_longitude);
        let lat = meta.lat.or(row.geo_latitude);
        if let (Some(lng), Some(lat)) = (lng, lat) {
            self.lng_sum += lng;
            self.lat_sum += lat;
            self.coord_count = self.coord_count.saturating_add(1);
        }

        if self.region.is_none() {
            self.region = meta.region;
        }

        if let Some(device_names) = row.device_names {
            for name in device_names {
                let name = normalize_gpu_model_name(&name);
                if !name.is_empty() {
                    *self.gpu_counts.entry(name).or_insert(0) += 1;
                }
            }
        }
    }

    fn into_node(self) -> Option<ComputeMapNode> {
        if self.node_count == 0 || self.coord_count == 0 {
            return None;
        }

        let status = if self.warning_count > 0 {
            CityStatus::Warning
        } else if self.online_count > 0 {
            CityStatus::Online
        } else if self.maintenance_count == self.node_count {
            CityStatus::Maintenance
        } else {
            CityStatus::Offline
        };

        Some(ComputeMapNode {
            id: self.id,
            name: self.name,
            lng: round4(self.lng_sum / self.coord_count as f64),
            lat: round4(self.lat_sum / self.coord_count as f64),
            node_count: self.node_count,
            tflops: self.tflops,
            gpu_model: top_gpu_models(self.gpu_counts),
            region: self.region,
            status: status.as_str().to_string(),
        })
    }
}

#[derive(Debug, Clone)]
struct CityMeta {
    id: String,
    name: String,
    region: Option<String>,
    lng: Option<f64>,
    lat: Option<f64>,
}

pub async fn get_compute_map(pool: &Pool<Postgres>) -> Result<ComputeMapResponse> {
    let rows = sqlx::query_as::<_, ComputeMapAssetRow>(
        r#"
        SELECT
            ga.client_status,
            ga.valid_status,
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
            di.avg_device_gpuusage,
            COALESCE(di.device_names, ARRAY[]::TEXT[]) AS device_names
        FROM gpu_assets ga
        LEFT JOIN system_info si ON si.client_id = ga.client_id
        LEFT JOIN (
            SELECT
                client_id,
                AVG(device_gpuusage)::FLOAT8 AS avg_device_gpuusage,
                ARRAY_AGG(DISTINCT device_name) FILTER (WHERE device_name IS NOT NULL AND device_name <> '') AS device_names
            FROM device_info
            GROUP BY client_id
        ) di ON di.client_id = ga.client_id
        WHERE COALESCE(ga.valid_status, 'valid') IN ('valid', 'warning')
          AND ga.geo_city IS NOT NULL
          AND ga.geo_latitude IS NOT NULL
          AND ga.geo_longitude IS NOT NULL
          AND (
            ga.geo_country IS NULL
            OR LOWER(ga.geo_country) IN ('china', 'cn', '中国', 'hong kong', 'hong kong sar', 'hk', 'macao', 'macau', 'taiwan')
          )
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut cities: HashMap<String, CityAggregate> = HashMap::new();

    for row in rows {
        let Some(raw_city) = row.geo_city.as_deref() else {
            continue;
        };
        let meta = city_meta(raw_city, row.geo_region.as_deref());
        let entry = cities.entry(meta.id.clone()).or_insert_with(|| {
            CityAggregate::new(meta.id.clone(), meta.name.clone(), meta.region.clone())
        });
        entry.add_row(row, meta);
    }

    let mut online_nodes = 0_u32;
    let mut used_nodes = 0_u32;
    let mut nodes: Vec<ComputeMapNode> = Vec::with_capacity(cities.len());
    for city in cities.into_values() {
        let city_online_count = city.online_count;
        let city_used_count = city.used_count;
        if let Some(node) = city.into_node() {
            online_nodes = online_nodes.saturating_add(city_online_count);
            used_nodes = used_nodes.saturating_add(city_used_count);
            nodes.push(node);
        }
    }
    nodes.sort_by(|a, b| {
        b.node_count
            .cmp(&a.node_count)
            .then_with(|| b.tflops.cmp(&a.tflops))
            .then_with(|| a.id.cmp(&b.id))
    });

    let token_today = token_usage::get_token_usage_summary_today(pool, None).await?;
    let token_latest = token_usage::get_token_usage_latest_window(
        pool,
        token_usage::REALTIME_TPS_WINDOW_SECONDS,
        None,
    )
    .await?;
    let summary = ComputeMapSummary {
        online_nodes,
        total_tflops: nodes.iter().map(|node| node.tflops).sum(),
        token_tps: token_usage::tokens_per_second(
            token_latest.total_tokens,
            token_usage::REALTIME_TPS_WINDOW_SECONDS,
        ),
        today_token_total: token_today.total_tokens.max(0) as u64,
        today_token_unit: "tokens".to_string(),
        used_nodes,
    };

    Ok(ComputeMapResponse {
        summary: Some(summary),
        nodes,
        links: Vec::<ComputeMapLink>::new(),
    })
}

fn normalize_status(client_status: Option<&str>, valid_status: Option<&str>) -> CityStatus {
    match valid_status.map(|s| s.trim().to_ascii_lowercase()) {
        Some(status) if status == "warning" || status == "invalid" => return CityStatus::Warning,
        _ => {}
    }

    match client_status
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("online") | Some("active") => CityStatus::Online,
        Some("warning") | Some("error") => CityStatus::Warning,
        Some("maintenance") => CityStatus::Maintenance,
        _ => CityStatus::Offline,
    }
}

fn is_used_node(row: &ComputeMapAssetRow) -> bool {
    if normalize_status(row.client_status.as_deref(), row.valid_status.as_deref())
        != CityStatus::Online
    {
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

fn current_load(row: &ComputeMapAssetRow) -> u8 {
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

fn city_meta(raw_city: &str, raw_region: Option<&str>) -> CityMeta {
    let normalized = normalize_lookup_key(raw_city);
    match normalized.as_str() {
        "beijing" | "北京" | "北京市" => CityMeta {
            id: "beijing".to_string(),
            name: "北京".to_string(),
            region: Some("华北算力区".to_string()),
            lng: Some(116.4074),
            lat: Some(39.9042),
        },
        "shanghai" | "上海" | "上海市" => CityMeta {
            id: "shanghai".to_string(),
            name: "上海".to_string(),
            region: Some("华东算力区".to_string()),
            lng: Some(121.4737),
            lat: Some(31.2304),
        },
        "shenzhen" | "深圳" | "深圳市" => CityMeta {
            id: "shenzhen".to_string(),
            name: "深圳".to_string(),
            region: Some("华南算力区".to_string()),
            lng: Some(114.0579),
            lat: Some(22.5431),
        },
        "guangzhou" | "广州" | "广州市" => CityMeta {
            id: "guangzhou".to_string(),
            name: "广州".to_string(),
            region: Some("华南算力区".to_string()),
            lng: Some(113.2644),
            lat: Some(23.1291),
        },
        "hangzhou" | "杭州" | "杭州市" => CityMeta {
            id: "hangzhou".to_string(),
            name: "杭州".to_string(),
            region: Some("华东算力区".to_string()),
            lng: Some(120.1551),
            lat: Some(30.2741),
        },
        "nanjing" | "南京" | "南京市" => CityMeta {
            id: "nanjing".to_string(),
            name: "南京".to_string(),
            region: Some("华东算力区".to_string()),
            lng: Some(118.7969),
            lat: Some(32.0603),
        },
        "chengdu" | "成都" | "成都市" => CityMeta {
            id: "chengdu".to_string(),
            name: "成都".to_string(),
            region: Some("西南算力区".to_string()),
            lng: Some(104.0665),
            lat: Some(30.5728),
        },
        "chongqing" | "重庆" | "重庆市" => CityMeta {
            id: "chongqing".to_string(),
            name: "重庆".to_string(),
            region: Some("西南算力区".to_string()),
            lng: Some(106.5516),
            lat: Some(29.563),
        },
        "wuhan" | "武汉" | "武汉市" => CityMeta {
            id: "wuhan".to_string(),
            name: "武汉".to_string(),
            region: Some("华中算力区".to_string()),
            lng: Some(114.3054),
            lat: Some(30.5931),
        },
        "xian" | "xi'an" | "西安" | "西安市" => CityMeta {
            id: "xian".to_string(),
            name: "西安".to_string(),
            region: Some("西北算力区".to_string()),
            lng: Some(108.9398),
            lat: Some(34.3416),
        },
        "tianjin" | "天津" | "天津市" => CityMeta {
            id: "tianjin".to_string(),
            name: "天津".to_string(),
            region: Some("华北算力区".to_string()),
            lng: Some(117.2009),
            lat: Some(39.0842),
        },
        "qingdao" | "青岛" | "青岛市" => CityMeta {
            id: "qingdao".to_string(),
            name: "青岛".to_string(),
            region: Some("华东算力区".to_string()),
            lng: Some(120.3826),
            lat: Some(36.0671),
        },
        "suzhou" | "苏州" | "苏州市" => CityMeta {
            id: "suzhou".to_string(),
            name: "苏州".to_string(),
            region: Some("华东算力区".to_string()),
            lng: Some(120.5853),
            lat: Some(31.2989),
        },
        "changsha" | "长沙" | "长沙市" => CityMeta {
            id: "changsha".to_string(),
            name: "长沙".to_string(),
            region: Some("华中算力区".to_string()),
            lng: Some(112.9388),
            lat: Some(28.2282),
        },
        "zhengzhou" | "郑州" | "郑州市" => CityMeta {
            id: "zhengzhou".to_string(),
            name: "郑州".to_string(),
            region: Some("华中算力区".to_string()),
            lng: Some(113.6254),
            lat: Some(34.7466),
        },
        "jinan" | "济南" | "济南市" => CityMeta {
            id: "jinan".to_string(),
            name: "济南".to_string(),
            region: Some("华东算力区".to_string()),
            lng: Some(117.1201),
            lat: Some(36.6512),
        },
        "shenyang" | "沈阳" | "沈阳市" => CityMeta {
            id: "shenyang".to_string(),
            name: "沈阳".to_string(),
            region: Some("东北算力区".to_string()),
            lng: Some(123.4315),
            lat: Some(41.8057),
        },
        "dalian" | "大连" | "大连市" => CityMeta {
            id: "dalian".to_string(),
            name: "大连".to_string(),
            region: Some("东北算力区".to_string()),
            lng: Some(121.6147),
            lat: Some(38.914),
        },
        "harbin" | "哈尔滨" | "哈尔滨市" => CityMeta {
            id: "harbin".to_string(),
            name: "哈尔滨".to_string(),
            region: Some("东北算力区".to_string()),
            lng: Some(126.6425),
            lat: Some(45.7567),
        },
        "kunming" | "昆明" | "昆明市" => CityMeta {
            id: "kunming".to_string(),
            name: "昆明".to_string(),
            region: Some("西南算力区".to_string()),
            lng: Some(102.8329),
            lat: Some(24.8801),
        },
        "guiyang" | "贵阳" | "贵阳市" => CityMeta {
            id: "guiyang".to_string(),
            name: "贵阳".to_string(),
            region: Some("西南算力区".to_string()),
            lng: Some(106.6302),
            lat: Some(26.6477),
        },
        "lanzhou" | "兰州" | "兰州市" => CityMeta {
            id: "lanzhou".to_string(),
            name: "兰州".to_string(),
            region: Some("西北算力区".to_string()),
            lng: Some(103.8343),
            lat: Some(36.0611),
        },
        "urumqi" | "乌鲁木齐" | "乌鲁木齐市" => CityMeta {
            id: "urumqi".to_string(),
            name: "乌鲁木齐".to_string(),
            region: Some("西北算力区".to_string()),
            lng: Some(87.6168),
            lat: Some(43.8256),
        },
        "hongkong" | "hong kong" | "香港" | "香港特别行政区" => CityMeta {
            id: "hongkong".to_string(),
            name: "香港".to_string(),
            region: Some("华南算力区".to_string()),
            lng: Some(114.1694),
            lat: Some(22.3193),
        },
        _ => {
            let name = raw_city.trim();
            CityMeta {
                id: slugify_city(name),
                name: if name.is_empty() {
                    "未知城市".to_string()
                } else {
                    name.to_string()
                },
                region: Some(region_from_geo(raw_region)),
                lng: None,
                lat: None,
            }
        }
    }
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

fn region_from_geo(raw_region: Option<&str>) -> String {
    match raw_region.map(normalize_lookup_key).as_deref() {
        Some("beijing")
        | Some("tianjin")
        | Some("hebei")
        | Some("shanxi")
        | Some("inner mongolia")
        | Some("内蒙古")
        | Some("河北")
        | Some("山西")
        | Some("北京")
        | Some("天津") => "华北算力区".to_string(),
        Some("shanghai") | Some("jiangsu") | Some("zhejiang") | Some("anhui") | Some("fujian")
        | Some("jiangxi") | Some("shandong") | Some("上海") | Some("江苏") | Some("浙江")
        | Some("安徽") | Some("福建") | Some("江西") | Some("山东") => {
            "华东算力区".to_string()
        }
        Some("guangdong") | Some("guangxi") | Some("hainan") | Some("hong kong")
        | Some("macau") | Some("广东") | Some("广西") | Some("海南") | Some("香港")
        | Some("澳门") => "华南算力区".to_string(),
        Some("henan") | Some("hubei") | Some("hunan") | Some("河南") | Some("湖北")
        | Some("湖南") => "华中算力区".to_string(),
        Some("chongqing") | Some("sichuan") | Some("guizhou") | Some("yunnan") | Some("tibet")
        | Some("重庆") | Some("四川") | Some("贵州") | Some("云南") | Some("西藏") => {
            "西南算力区".to_string()
        }
        Some("shaanxi") | Some("gansu") | Some("qinghai") | Some("ningxia") | Some("xinjiang")
        | Some("陕西") | Some("甘肃") | Some("青海") | Some("宁夏") | Some("新疆") => {
            "西北算力区".to_string()
        }
        Some("liaoning") | Some("jilin") | Some("heilongjiang") | Some("辽宁") | Some("吉林")
        | Some("黑龙江") => "东北算力区".to_string(),
        _ => "未知算力区".to_string(),
    }
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

fn top_gpu_models(counts: HashMap<String, u32>) -> Option<String> {
    let mut items: Vec<(String, u32)> = counts.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let models: Vec<String> = items.into_iter().take(3).map(|(name, _)| name).collect();
    if models.is_empty() {
        None
    } else {
        Some(models.join("/"))
    }
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}
