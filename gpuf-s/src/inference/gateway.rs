use anyhow::Result;
use axum::{
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use sqlx::{Pool, Postgres};
use tower_http::cors::CorsLayer;
use tracing::{info,debug,error};
use rdkafka::producer::FutureProducer;

use crate::db::client::get_user_client_by_token;
use crate::inference::{InferenceScheduler, handlers};
use crate::handle::{ActiveClients};
use crate::util::protoc::{ClientId, RequestIDAndClientIDMessage};
use anyhow::anyhow;
use rdkafka::producer::FutureRecord;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct AuthContext {
    pub client_ids: Vec<ClientId>,
    pub access_level: i32,
    pub token: String,
}

/// Inference Gateway - Handles external API requests and routes them to Android devices
pub struct InferenceGateway {
    pub scheduler: Arc<InferenceScheduler>,
    pub db_pool: Arc<Pool<Postgres>>,
    pub producer: Arc<FutureProducer>,
}

impl InferenceGateway {
    pub fn new(scheduler: Arc<InferenceScheduler>, db_pool: Arc<Pool<Postgres>>, producer: Arc<FutureProducer>) -> Self {
        Self { scheduler, db_pool, producer }
    }
    #[allow(dead_code)]
    pub fn with_active_clients(active_clients: ActiveClients, db_pool: Arc<Pool<Postgres>>, producer: Arc<FutureProducer>) -> Self {
        let scheduler = Arc::new(InferenceScheduler::new(active_clients));
        Self { scheduler, db_pool, producer }
    }

    async fn auth_middleware(
        axum::extract::State(db_pool): axum::extract::State<Arc<Pool<Postgres>>>,
        req: Request<axum::body::Body>,
        next: Next,
    ) -> Response {
        if req.method() == axum::http::Method::OPTIONS {
            return next.run(req).await;
        }

        let provided = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(str::to_string);

        let Some(token) = provided else {
            error!("No Authorization header");
            return StatusCode::UNAUTHORIZED.into_response();
        };
        debug!("Received token: {}", token);
        match get_user_client_by_token(&db_pool, token.as_str()).await {
            Ok((client_ids, access_level)) => {
                let mut req = req;
                req.extensions_mut().insert(AuthContext {
                    client_ids,
                    access_level,
                    token,
                });
                next.run(req).await
            }
            Err(_) => StatusCode::UNAUTHORIZED.into_response(),
        }
    }

    /// Send request metrics to Kafka if access_level requires it
    pub async fn send_request_metrics(
        &self,
        request_id: Option<String>,
        chosen_client_id: ClientId,
        access_level: i32,
    ) -> Result<()> {
        // Skip if access_level is -1 (private API)
        if access_level != -1 {
            debug!("Send kafka key-value (request_id, client_id) pair");
            return Ok(());
        }

        // Share API: Send kafka key-value (request_id, client_id) pair
        if let Some(request_id_str) = request_id {
            let message = RequestIDAndClientIDMessage {
                request_id: hex::decode(request_id_str)?
                    .try_into()
                    .map_err(|_| anyhow!("Invalid client ID length"))?,
                client_id: chosen_client_id.0,
            };

            let request_message_bytes = serde_json::to_vec(&message).unwrap();

            self.producer
                .send(
                    FutureRecord::to("request-message")
                        .payload(&request_message_bytes)
                        .key(&chosen_client_id.to_string()),
                    Duration::from_secs(0),
                )
                .await
                .map_err(|(e, _)| anyhow!("Failed to send to Kafka: {}", e))?;
        }

        Ok(())
    }

    /// Run the inference gateway server
    pub async fn run(self: Arc<Self>, port: u16) -> Result<()> {
        let app = self.create_router().await;
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

        info!("Inference Gateway listening on port {}", port);
        axum::serve(listener, app).await.map_err(Into::into)
    }

    /// Create API router for inference endpoints
    pub async fn create_router(self: Arc<Self>) -> Router {
        let state = Arc::clone(&self);
        Router::new()
            // OpenAI Compatible Inference APIs
            .route("/v1/completions", post(handlers::handle_completion))
            .route("/v1/chat/completions", post(handlers::handle_chat_completion))
            .route("/v1/models", get(handlers::list_models))
            // Device Management APIs
            .route("/api/v1/devices", get(handlers::list_devices))
            .route("/api/v1/devices/:id/status", get(handlers::get_device_status))
            .route_layer(middleware::from_fn_with_state(self.db_pool.clone(), Self::auth_middleware))
            .layer(CorsLayer::permissive())
            .with_state(state)
    }
}
