use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::inference::{InferenceScheduler, handlers};
use crate::handle::{ActiveClients};

/// Inference Gateway - Handles external API requests and routes them to Android devices
pub struct InferenceGateway {
    pub scheduler: Arc<InferenceScheduler>,
}

impl InferenceGateway {
    pub fn new(scheduler: Arc<InferenceScheduler>) -> Self {
        Self { scheduler }
    }
    
    pub fn with_active_clients(active_clients: ActiveClients) -> Self {
        let scheduler = Arc::new(InferenceScheduler::new(active_clients));
        Self { scheduler }
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
            .layer(CorsLayer::permissive())
            .with_state(state)
    }
}
