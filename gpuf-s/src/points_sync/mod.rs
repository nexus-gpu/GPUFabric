use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, Row};
use std::time::Duration;
use tracing::{debug, error, info, warn};

const SYNC_LOCK_ID: i64 = 0x4750_5546_4352_4544; // GPUFCRED
const SOURCE: &str = "gpuf_compute";
const CREDIT_TYPE: &str = "earned";

#[derive(Clone, Debug)]
pub struct PointsSyncConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub service_token: String,
    pub batch_size: i64,
    pub settle_lag_days: i64,
    pub credit_scale: i64,
    pub request_timeout_secs: u64,
    pub max_attempts: i32,
}

impl PointsSyncConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            service_token: String::new(),
            batch_size: 100,
            settle_lag_days: 2,
            credit_scale: 100,
            request_timeout_secs: 10,
            max_attempts: 10,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.endpoint.trim().is_empty() {
            return Err(anyhow!("points sync endpoint is required when enabled"));
        }
        if self.service_token.trim().is_empty() {
            return Err(anyhow!(
                "points sync service token is required when enabled"
            ));
        }
        if self.batch_size <= 0 {
            return Err(anyhow!("points sync batch size must be positive"));
        }
        if self.settle_lag_days < 1 {
            return Err(anyhow!("points sync settle lag days must be at least 1"));
        }
        if self.credit_scale <= 0 {
            return Err(anyhow!("points sync credit scale must be positive"));
        }
        if self.request_timeout_secs == 0 {
            return Err(anyhow!("points sync request timeout must be positive"));
        }
        if self.max_attempts <= 0 {
            return Err(anyhow!("points sync max attempts must be positive"));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct PointsSyncWorker {
    db_pool: Pool<Postgres>,
    http_client: reqwest::Client,
    config: PointsSyncConfig,
}

#[derive(Debug)]
struct SyncItem {
    sync_key: String,
    user_id: i64,
    client_id_hex: String,
    device_index: i16,
    settle_date: NaiveDate,
    source_points: String,
    credit_amount: i32,
    attempts: i32,
}

#[derive(Debug, Serialize)]
struct GrantCreditRequest<'a> {
    user_id: i64,
    amount: i32,
    credit_type: &'a str,
    biz_no: &'a str,
    source: &'a str,
    description: String,
}

#[derive(Debug, Deserialize)]
struct GrantCreditResponse {
    success: bool,
    code: i32,
    message: String,
    data: Option<GrantCreditData>,
}

#[derive(Debug, Deserialize)]
struct GrantCreditData {
    #[serde(default)]
    transaction_id: i64,
    #[serde(default)]
    balance_after: i32,
}

impl PointsSyncWorker {
    pub fn new(db_pool: Pool<Postgres>, config: PointsSyncConfig) -> Result<Self> {
        config.validate()?;
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .context("failed to build points sync http client")?;

        Ok(Self {
            db_pool,
            http_client,
            config,
        })
    }

    pub async fn run_once(&self) -> Result<()> {
        if !self.config.enabled {
            debug!("points sync skipped because it is disabled");
            return Ok(());
        }

        let mut lock_conn = self
            .db_pool
            .acquire()
            .await
            .context("failed to acquire points sync lock connection")?;
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(SYNC_LOCK_ID)
            .fetch_one(&mut *lock_conn)
            .await
            .context("failed to acquire points sync advisory lock")?;
        if !locked {
            info!("points sync skipped because another worker is running");
            return Ok(());
        }

        let run_result: Result<()> = async {
            self.ensure_schema().await?;
            self.stage_ready_points().await?;
            let items = self.load_pending_items().await?;
            if items.is_empty() {
                debug!("points sync found no pending rows");
                return Ok(());
            }

            let mut success_count = 0usize;
            let mut failure_count = 0usize;
            for item in items {
                match self.grant_credit(&item).await {
                    Ok((transaction_id, balance_after)) => {
                        self.mark_synced(&item.sync_key, transaction_id, balance_after)
                            .await?;
                        success_count += 1;
                    }
                    Err(err) => {
                        warn!(
                            sync_key = %item.sync_key,
                            attempts = item.attempts + 1,
                            error = %err,
                            "points sync grant failed"
                        );
                        self.mark_failed(&item.sync_key, &err.to_string()).await?;
                        failure_count += 1;
                    }
                }
            }

            info!(success_count, failure_count, "points sync run completed");
            Ok(())
        }
        .await;

        match sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
            .bind(SYNC_LOCK_ID)
            .fetch_one(&mut *lock_conn)
            .await
        {
            Ok(true) => {}
            Ok(false) => warn!("points sync advisory lock was not held at release time"),
            Err(err) => warn!(error = %err, "failed to release points sync advisory lock"),
        }

        run_result
    }

    async fn ensure_schema(&self) -> Result<()> {
        let statements = [
            r#"
            CREATE TABLE IF NOT EXISTS device_points_credit_sync (
                sync_key TEXT PRIMARY KEY,
                user_id BIGINT NOT NULL,
                client_id BYTEA NOT NULL,
                client_id_hex CHAR(32) NOT NULL,
                device_index SMALLINT NOT NULL,
                settle_date DATE NOT NULL,
                source_points NUMERIC NOT NULL,
                credit_amount INTEGER NOT NULL,
                source_refreshed_at TIMESTAMPTZ NOT NULL,
                status VARCHAR(32) NOT NULL DEFAULT 'pending',
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                credit_transaction_id BIGINT,
                credit_balance_after INTEGER,
                synced_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
            r#"
            CREATE INDEX IF NOT EXISTS idx_device_points_credit_sync_status
            ON device_points_credit_sync (status, settle_date)
            "#,
            r#"
            CREATE INDEX IF NOT EXISTS idx_device_points_credit_sync_user_date
            ON device_points_credit_sync (user_id, settle_date)
            "#,
        ];

        for sql in statements {
            sqlx::query(sql).execute(&self.db_pool).await?;
        }
        Ok(())
    }

    async fn stage_ready_points(&self) -> Result<()> {
        let sql = r#"
            WITH ready_points AS (
                SELECT
                    concat('gpuf:device_points_daily:', dpd.date::text, ':', encode(dpd.client_id, 'hex'), ':', dpd.device_index::text) AS sync_key,
                    ga.user_id::BIGINT AS user_id,
                    dpd.client_id,
                    encode(dpd.client_id, 'hex') AS client_id_hex,
                    dpd.device_index,
                    dpd.date AS settle_date,
                    dpd.points AS source_points,
                    ROUND(dpd.points * $1)::NUMERIC AS credit_amount,
                    dpd.refreshed_at AS source_refreshed_at
                FROM device_points_daily dpd
                INNER JOIN gpu_assets ga ON ga.client_id = dpd.client_id
                WHERE dpd.points > 0
                  AND dpd.date <= (CURRENT_DATE - ($2::INTEGER * INTERVAL '1 day'))::DATE
                  AND ga.user_id ~ '^[0-9]{1,18}$'
            )
            INSERT INTO device_points_credit_sync (
                sync_key,
                user_id,
                client_id,
                client_id_hex,
                device_index,
                settle_date,
                source_points,
                credit_amount,
                source_refreshed_at,
                status,
                updated_at
            )
            SELECT
                sync_key,
                user_id,
                client_id,
                client_id_hex,
                device_index,
                settle_date,
                source_points,
                credit_amount::INTEGER,
                source_refreshed_at,
                'pending' AS status,
                NOW() AS updated_at
            FROM ready_points
            WHERE credit_amount > 0
              AND credit_amount <= 2147483647
            ON CONFLICT (sync_key) DO UPDATE SET
                user_id = EXCLUDED.user_id,
                source_points = EXCLUDED.source_points,
                credit_amount = EXCLUDED.credit_amount,
                source_refreshed_at = EXCLUDED.source_refreshed_at,
                status = 'pending',
                attempts = 0,
                last_error = NULL,
                updated_at = NOW()
            WHERE device_points_credit_sync.status = 'pending'
              AND device_points_credit_sync.attempts = 0
              AND (
                device_points_credit_sync.source_points IS DISTINCT FROM EXCLUDED.source_points
                OR device_points_credit_sync.credit_amount IS DISTINCT FROM EXCLUDED.credit_amount
                OR device_points_credit_sync.user_id IS DISTINCT FROM EXCLUDED.user_id
              );
        "#;

        let result = sqlx::query(sql)
            .bind(self.config.credit_scale)
            .bind(self.config.settle_lag_days as i32)
            .execute(&self.db_pool)
            .await?;
        debug!(
            rows = result.rows_affected(),
            "staged ready device points for credit sync"
        );
        Ok(())
    }

    async fn load_pending_items(&self) -> Result<Vec<SyncItem>> {
        let sql = r#"
            SELECT
                sync_key,
                user_id,
                client_id_hex,
                device_index,
                settle_date,
                source_points::TEXT AS source_points,
                credit_amount,
                attempts
            FROM device_points_credit_sync
            WHERE status IN ('pending', 'failed')
              AND attempts < $1
            ORDER BY settle_date ASC, sync_key ASC
            LIMIT $2
        "#;

        let rows = sqlx::query(sql)
            .bind(self.config.max_attempts)
            .bind(self.config.batch_size)
            .fetch_all(&self.db_pool)
            .await?;

        rows.into_iter()
            .map(|row| {
                Ok(SyncItem {
                    sync_key: row.try_get("sync_key")?,
                    user_id: row.try_get("user_id")?,
                    client_id_hex: row.try_get("client_id_hex")?,
                    device_index: row.try_get("device_index")?,
                    settle_date: row.try_get("settle_date")?,
                    source_points: row.try_get("source_points")?,
                    credit_amount: row.try_get("credit_amount")?,
                    attempts: row.try_get("attempts")?,
                })
            })
            .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
            .map_err(Into::into)
    }

    async fn grant_credit(&self, item: &SyncItem) -> Result<(i64, i32)> {
        let request = GrantCreditRequest {
            user_id: item.user_id,
            amount: item.credit_amount,
            credit_type: CREDIT_TYPE,
            biz_no: &item.sync_key,
            source: SOURCE,
            description: format!(
                "GPUFabric compute points {} client={} device={} points={}",
                item.settle_date, item.client_id_hex, item.device_index, item.source_points
            ),
        };

        let response = self
            .http_client
            .post(self.config.endpoint.trim())
            .header("x-service-token", self.config.service_token.trim())
            .json(&request)
            .send()
            .await
            .context("failed to call credit grant endpoint")?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(anyhow!(
                "credit grant endpoint rejected service token: {}",
                status
            ));
        }
        if !status.is_success() {
            return Err(anyhow!("credit grant endpoint returned status {}", status));
        }

        let payload: GrantCreditResponse = response
            .json()
            .await
            .context("failed to decode credit grant response")?;
        if !payload.success {
            return Err(anyhow!(
                "credit grant failed code={} message={}",
                payload.code,
                payload.message
            ));
        }

        let data = payload
            .data
            .ok_or_else(|| anyhow!("credit grant response missing data"))?;
        Ok((data.transaction_id, data.balance_after))
    }

    async fn mark_synced(
        &self,
        sync_key: &str,
        transaction_id: i64,
        balance_after: i32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE device_points_credit_sync
            SET status = 'synced',
                credit_transaction_id = $2,
                credit_balance_after = $3,
                synced_at = NOW(),
                last_error = NULL,
                updated_at = NOW()
            WHERE sync_key = $1
            "#,
        )
        .bind(sync_key)
        .bind(transaction_id)
        .bind(balance_after)
        .execute(&self.db_pool)
        .await?;
        Ok(())
    }

    async fn mark_failed(&self, sync_key: &str, error: &str) -> Result<()> {
        let truncated = if error.len() > 2000 {
            error.chars().take(2000).collect::<String>()
        } else {
            error.to_string()
        };
        sqlx::query(
            r#"
            UPDATE device_points_credit_sync
            SET status = 'failed',
                attempts = attempts + 1,
                last_error = $2,
                updated_at = NOW()
            WHERE sync_key = $1
            "#,
        )
        .bind(sync_key)
        .bind(&truncated)
        .execute(&self.db_pool)
        .await?;
        Ok(())
    }
}

pub async fn run_after_points_refresh(db_pool: Pool<Postgres>, config: PointsSyncConfig) {
    if !config.enabled {
        return;
    }

    match PointsSyncWorker::new(db_pool, config) {
        Ok(worker) => {
            if let Err(err) = worker.run_once().await {
                error!(error = %err, "points sync run failed");
            }
        }
        Err(err) => {
            error!(error = %err, "points sync configuration invalid");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Mutex};
    use uuid::Uuid;

    #[derive(Clone)]
    struct MockGrantState {
        received: Arc<Mutex<Vec<Value>>>,
    }

    #[tokio::test]
    async fn sync_worker_imports_settled_points_once_with_mock_credit_service() -> Result<()> {
        let Ok(database_url) = std::env::var("GPUF_POINTS_SYNC_TEST_DATABASE_URL") else {
            eprintln!("skipping postgres-backed points sync test; GPUF_POINTS_SYNC_TEST_DATABASE_URL is not set");
            return Ok(());
        };

        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await?;
        let schema = format!("points_sync_test_{}", Uuid::new_v4().simple());
        sqlx::query(&format!(r#"CREATE SCHEMA "{}""#, schema))
            .execute(&admin_pool)
            .await?;

        let test_url = with_search_path(&database_url, &schema);
        let db_pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&test_url)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE gpu_assets (
                user_id VARCHAR,
                client_id BYTEA PRIMARY KEY
            )
            "#,
        )
        .execute(&db_pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE device_points_daily (
                client_id BYTEA NOT NULL,
                device_index SMALLINT NOT NULL,
                date DATE NOT NULL,
                points NUMERIC NOT NULL,
                refreshed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (client_id, device_index, date)
            )
            "#,
        )
        .execute(&db_pool)
        .await?;

        let client_id = vec![7u8; 16];
        sqlx::query("INSERT INTO gpu_assets (user_id, client_id) VALUES ('42', $1)")
            .bind(&client_id)
            .execute(&db_pool)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO device_points_daily (client_id, device_index, date, points)
            VALUES ($1, 0, (CURRENT_DATE - INTERVAL '3 days')::DATE, 10.2)
            "#,
        )
        .bind(&client_id)
        .execute(&db_pool)
        .await?;

        let state = MockGrantState {
            received: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/grant", post(mock_grant_credit))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let worker = PointsSyncWorker::new(
            db_pool.clone(),
            PointsSyncConfig {
                enabled: true,
                endpoint: format!("http://{addr}/grant"),
                service_token: "test-token".to_string(),
                batch_size: 10,
                settle_lag_days: 1,
                credit_scale: 100,
                request_timeout_secs: 5,
                max_attempts: 3,
            },
        )?;

        worker.run_once().await?;
        worker.run_once().await?;

        let received = state.received.lock().await;
        assert_eq!(received.len(), 1);
        assert_eq!(received[0]["user_id"], json!(42));
        assert_eq!(received[0]["amount"], json!(1020));
        assert_eq!(received[0]["credit_type"], json!("earned"));
        assert_eq!(received[0]["source"], json!("gpuf_compute"));

        let (status, amount, transaction_id): (String, i32, Option<i64>) = sqlx::query_as(
            "SELECT status, credit_amount, credit_transaction_id FROM device_points_credit_sync",
        )
        .fetch_one(&db_pool)
        .await?;
        assert_eq!(status, "synced");
        assert_eq!(amount, 1020);
        assert_eq!(transaction_id, Some(9001));

        drop(received);
        drop(db_pool);
        sqlx::query(&format!(r#"DROP SCHEMA "{}" CASCADE"#, schema))
            .execute(&admin_pool)
            .await?;

        Ok(())
    }

    async fn mock_grant_credit(
        State(state): State<MockGrantState>,
        headers: HeaderMap,
        Json(payload): Json<Value>,
    ) -> (axum::http::StatusCode, Json<Value>) {
        if headers.get("x-service-token").and_then(|v| v.to_str().ok()) != Some("test-token") {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                Json(json!({
                    "success": false,
                    "code": 20006,
                    "message": "unauthorized"
                })),
            );
        }

        state.received.lock().await.push(payload);
        (
            axum::http::StatusCode::OK,
            Json(json!({
                "success": true,
                "code": 20000,
                "message": "success",
                "data": {
                    "transaction_id": 9001,
                    "balance_after": 1020
                }
            })),
        )
    }

    fn with_search_path(database_url: &str, schema: &str) -> String {
        let sep = if database_url.contains('?') { '&' } else { '?' };
        format!("{database_url}{sep}options=-csearch_path%3D{schema}")
    }
}
