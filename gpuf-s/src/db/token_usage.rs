use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{postgres::Postgres, FromRow, Pool};

use crate::util::protoc::ClientId;

pub const REALTIME_TPS_WINDOW_SECONDS: u32 = 10;

#[derive(Debug, Clone)]
pub struct TokenUsageInsert {
    pub request_id: Option<String>,
    pub token_hash: Option<String>,
    pub client_id: ClientId,
    pub model: String,
    pub endpoint: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub success: bool,
    pub stream: bool,
}

#[derive(Debug, FromRow)]
pub struct TokenUsageWindowRow {
    pub bucket: DateTime<Utc>,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, FromRow)]
pub struct TokenUsageSummaryRow {
    pub total_tokens: i64,
}

#[derive(Debug, FromRow)]
pub struct TokenUsageIoSummaryRow {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
}

pub async fn ensure_token_usage_schema(pool: &Pool<Postgres>) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS inference_token_usage (
            id BIGSERIAL PRIMARY KEY,
            request_id VARCHAR,
            token_hash VARCHAR,
            client_id BYTEA NOT NULL,
            model VARCHAR NOT NULL,
            endpoint VARCHAR NOT NULL,
            prompt_tokens BIGINT NOT NULL DEFAULT 0,
            completion_tokens BIGINT NOT NULL DEFAULT 0,
            total_tokens BIGINT NOT NULL DEFAULT 0,
            success BOOLEAN NOT NULL DEFAULT TRUE,
            stream BOOLEAN NOT NULL DEFAULT FALSE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_inference_token_usage_created_at
        ON inference_token_usage (created_at DESC)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_inference_token_usage_client_created
        ON inference_token_usage (client_id, created_at DESC)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_inference_token_usage_request_endpoint
        ON inference_token_usage (request_id, token_hash, endpoint)
        WHERE request_id IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn insert_token_usage(pool: &Pool<Postgres>, usage: TokenUsageInsert) -> Result<()> {
    let prompt_tokens = i64::from(usage.prompt_tokens);
    let completion_tokens = i64::from(usage.completion_tokens);
    let total_tokens = prompt_tokens.saturating_add(completion_tokens);
    let request_id = usage.request_id.and_then(|value| {
        let value = value.trim().to_string();
        (!value.is_empty()).then_some(value)
    });

    sqlx::query(
        r#"
        INSERT INTO inference_token_usage (
            request_id,
            token_hash,
            client_id,
            model,
            endpoint,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            success,
            stream
        )
        SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10
        WHERE $1::VARCHAR IS NULL
           OR NOT EXISTS (
                SELECT 1
                FROM inference_token_usage
                WHERE request_id = $1
                  AND COALESCE(token_hash, '') = COALESCE($2, '')
                  AND endpoint = $5
                LIMIT 1
           )
        "#,
    )
    .bind(request_id)
    .bind(usage.token_hash)
    .bind(usage.client_id.0.as_slice())
    .bind(usage.model)
    .bind(usage.endpoint)
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(total_tokens)
    .bind(usage.success)
    .bind(usage.stream)
    .execute(pool)
    .await?;

    Ok(())
}

pub fn tokens_per_second(total_tokens: i64, window_seconds: u32) -> f64 {
    let total_tokens = total_tokens.max(0) as f64;
    let window_seconds = f64::from(window_seconds.max(1));
    total_tokens / window_seconds
}

pub async fn get_token_usage_summary_today(
    pool: &Pool<Postgres>,
    region: Option<&str>,
) -> Result<TokenUsageSummaryRow> {
    let row = sqlx::query_as::<_, TokenUsageSummaryRow>(
        r#"
        SELECT
            COALESCE(SUM(itu.total_tokens), 0)::BIGINT AS total_tokens
        FROM inference_token_usage itu
        LEFT JOIN gpu_assets ga ON ga.client_id = itu.client_id
        WHERE itu.success = TRUE
          AND itu.created_at >= date_trunc('day', NOW())
          AND (
            $1::TEXT IS NULL
            OR LOWER(COALESCE(ga.geo_city, '')) = LOWER($1)
            OR LOWER(COALESCE(ga.geo_region, '')) = LOWER($1)
          )
        "#,
    )
    .bind(region.filter(|value| !value.trim().is_empty()))
    .fetch_one(pool)
    .await?;

    Ok(row)
}

pub async fn get_token_usage_total_in_range(
    pool: &Pool<Postgres>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    region: Option<&str>,
) -> Result<TokenUsageSummaryRow> {
    let row = sqlx::query_as::<_, TokenUsageSummaryRow>(
        r#"
        SELECT
            COALESCE(SUM(itu.total_tokens), 0)::BIGINT AS total_tokens
        FROM inference_token_usage itu
        LEFT JOIN gpu_assets ga ON ga.client_id = itu.client_id
        WHERE itu.success = TRUE
          AND itu.created_at >= $1
          AND itu.created_at < $2
          AND (
            $3::TEXT IS NULL
            OR LOWER(COALESCE(ga.geo_city, '')) = LOWER($3)
            OR LOWER(COALESCE(ga.geo_region, '')) = LOWER($3)
          )
        "#,
    )
    .bind(from)
    .bind(to)
    .bind(region.filter(|value| !value.trim().is_empty()))
    .fetch_one(pool)
    .await?;

    Ok(row)
}

pub async fn get_token_usage_latest_window(
    pool: &Pool<Postgres>,
    window_seconds: u32,
    region: Option<&str>,
) -> Result<TokenUsageSummaryRow> {
    let row = sqlx::query_as::<_, TokenUsageSummaryRow>(
        r#"
        SELECT
            COALESCE(SUM(itu.total_tokens), 0)::BIGINT AS total_tokens
        FROM inference_token_usage itu
        LEFT JOIN gpu_assets ga ON ga.client_id = itu.client_id
        WHERE itu.success = TRUE
          AND itu.created_at >= NOW() - ($1::INTEGER * INTERVAL '1 second')
          AND (
            $2::TEXT IS NULL
            OR LOWER(COALESCE(ga.geo_city, '')) = LOWER($2)
            OR LOWER(COALESCE(ga.geo_region, '')) = LOWER($2)
          )
        "#,
    )
    .bind(window_seconds as i32)
    .bind(region.filter(|value| !value.trim().is_empty()))
    .fetch_one(pool)
    .await?;

    Ok(row)
}

pub async fn get_token_usage_io_latest_window(
    pool: &Pool<Postgres>,
    window_seconds: u32,
    region: Option<&str>,
) -> Result<TokenUsageIoSummaryRow> {
    let row = sqlx::query_as::<_, TokenUsageIoSummaryRow>(
        r#"
        SELECT
            COALESCE(SUM(itu.prompt_tokens), 0)::BIGINT AS input_tokens,
            COALESCE(SUM(itu.completion_tokens), 0)::BIGINT AS output_tokens,
            COALESCE(SUM(itu.total_tokens), 0)::BIGINT AS total_tokens
        FROM inference_token_usage itu
        LEFT JOIN gpu_assets ga ON ga.client_id = itu.client_id
        WHERE itu.success = TRUE
          AND itu.created_at >= NOW() - ($1::INTEGER * INTERVAL '1 second')
          AND (
            $2::TEXT IS NULL
            OR LOWER(COALESCE(ga.geo_city, '')) = LOWER($2)
            OR LOWER(COALESCE(ga.geo_region, '')) = LOWER($2)
          )
        "#,
    )
    .bind(window_seconds as i32)
    .bind(region.filter(|value| !value.trim().is_empty()))
    .fetch_one(pool)
    .await?;

    Ok(row)
}

pub async fn get_token_usage_points(
    pool: &Pool<Postgres>,
    window_seconds: u32,
    interval_seconds: u32,
    region: Option<&str>,
) -> Result<Vec<TokenUsageWindowRow>> {
    let rows = sqlx::query_as::<_, TokenUsageWindowRow>(
        r#"
        WITH params AS (
            SELECT
                GREATEST($1::INTEGER, 1) AS window_seconds,
                GREATEST($2::INTEGER, 1) AS interval_seconds,
                $3::TEXT AS region
        ),
        buckets AS (
            SELECT generate_series(
                to_timestamp(floor(extract(epoch FROM NOW() - (params.window_seconds || ' seconds')::INTERVAL) / params.interval_seconds) * params.interval_seconds),
                to_timestamp(floor(extract(epoch FROM NOW()) / params.interval_seconds) * params.interval_seconds),
                (params.interval_seconds || ' seconds')::INTERVAL
            ) AS bucket
            FROM params
        ),
        usage AS (
            SELECT
                to_timestamp(floor(extract(epoch FROM itu.created_at) / params.interval_seconds) * params.interval_seconds) AS bucket,
                SUM(itu.prompt_tokens)::BIGINT AS input_tokens,
                SUM(itu.completion_tokens)::BIGINT AS output_tokens
            FROM inference_token_usage itu
            CROSS JOIN params
            LEFT JOIN gpu_assets ga ON ga.client_id = itu.client_id
            WHERE itu.success = TRUE
              AND itu.created_at >= NOW() - (params.window_seconds || ' seconds')::INTERVAL
              AND (
                params.region IS NULL
                OR LOWER(COALESCE(ga.geo_city, '')) = LOWER(params.region)
                OR LOWER(COALESCE(ga.geo_region, '')) = LOWER(params.region)
              )
            GROUP BY 1
        )
        SELECT
            buckets.bucket,
            COALESCE(usage.input_tokens, 0)::BIGINT AS input_tokens,
            COALESCE(usage.output_tokens, 0)::BIGINT AS output_tokens
        FROM buckets
        LEFT JOIN usage ON usage.bucket = buckets.bucket
        ORDER BY buckets.bucket
        "#,
    )
    .bind(window_seconds as i32)
    .bind(interval_seconds as i32)
    .bind(region.filter(|value| !value.trim().is_empty()))
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
