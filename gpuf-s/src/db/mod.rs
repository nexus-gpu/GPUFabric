pub mod apk;
pub mod client;
pub mod models;
pub mod stats;

const GPU_ASSETS_TABLE: &str = "gpu_assets";
const HEARTBEAT_TABLE: &str = "heartbeat";
const DEVICE_INFO_TABLE: &str = "device_info";
const SYSTEM_INFO_TABLE: &str = "system_info";
const APK_VERSIONS_TABLE: &str = "apk_versions";
#[allow(dead_code)]
const CLIENT_MODELS_TABLE: &str = "client_models";
const CLIENT_DAILY_STATS_TABLE: &str = "client_daily_stats";
const DEVICE_DAILY_STATS_TABLE: &str = "device_daily_stats";
