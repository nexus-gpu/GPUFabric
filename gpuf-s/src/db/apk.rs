use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Pool, Postgres, QueryBuilder};

use crate::db::APK_VERSIONS_TABLE;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApkVersion {
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

pub async fn upsert_apk_version(
    pool: &Pool<Postgres>,
    package_name: &str,
    version_name: &str,
    version_code: i64,
    download_url: &str,
    channel: Option<&str>,
    min_os_version: Option<&str>,
    sha256: Option<&str>,
    file_size_bytes: Option<i64>,
    is_active: Option<bool>,
    released_at: Option<DateTime<Utc>>,
) -> Result<ApkVersion, sqlx::Error> {
    sqlx::query_as(&format!(
        r#"
            INSERT INTO {table} (
                package_name,
                version_name,
                version_code,
                download_url,
                channel,
                min_os_version,
                sha256,
                file_size_bytes,
                is_active,
                released_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, COALESCE($9, TRUE), $10)
            ON CONFLICT (package_name, version_code)
            DO UPDATE SET
                version_name = EXCLUDED.version_name,
                download_url = EXCLUDED.download_url,
                channel = COALESCE(EXCLUDED.channel, {table}.channel),
                min_os_version = COALESCE(EXCLUDED.min_os_version, {table}.min_os_version),
                sha256 = COALESCE(EXCLUDED.sha256, {table}.sha256),
                file_size_bytes = COALESCE(EXCLUDED.file_size_bytes, {table}.file_size_bytes),
                is_active = COALESCE($9, {table}.is_active),
                released_at = COALESCE(EXCLUDED.released_at, {table}.released_at),
                updated_at = NOW()
            RETURNING *
            "#,
        table = APK_VERSIONS_TABLE
    ))
    .bind(package_name)
    .bind(version_name)
    .bind(version_code)
    .bind(download_url)
    .bind(channel)
    .bind(min_os_version)
    .bind(sha256)
    .bind(file_size_bytes)
    .bind(is_active)
    .bind(released_at)
    .fetch_one(pool)
    .await
}

pub async fn get_apk_version(
    pool: &Pool<Postgres>,
    package_name: &str,
    version_code: i64,
) -> Result<Option<ApkVersion>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT * FROM {table} WHERE package_name = $1 AND version_code = $2",
        table = APK_VERSIONS_TABLE
    ))
    .bind(package_name)
    .bind(version_code)
    .fetch_optional(pool)
    .await
}

pub async fn list_apk_versions(
    pool: &Pool<Postgres>,
    package_name: Option<&str>,
    channel: Option<&str>,
    is_active: Option<bool>,
    limit: Option<u32>,
) -> Result<Vec<ApkVersion>, sqlx::Error> {
    let mut query_builder = QueryBuilder::<Postgres>::new("SELECT * FROM ");
    query_builder.push(APK_VERSIONS_TABLE).push(" WHERE 1=1");

    if let Some(package_name) = package_name {
        query_builder
            .push(" AND package_name = ")
            .push_bind(package_name);
    }

    if let Some(channel) = channel {
        query_builder.push(" AND channel = ").push_bind(channel);
    }

    if let Some(is_active) = is_active {
        query_builder.push(" AND is_active = ").push_bind(is_active);
    }

    query_builder.push(" ORDER BY version_code DESC, released_at DESC NULLS LAST, created_at DESC");

    let limit = limit.unwrap_or(50).min(200) as i64;
    query_builder.push(" LIMIT ").push_bind(limit);

    query_builder
        .build_query_as::<ApkVersion>()
        .fetch_all(pool)
        .await
}
