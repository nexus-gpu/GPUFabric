use super::*;

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};

use crate::api_server::{apk, client, models};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

#[allow(dead_code)] // API server utility methods
impl ApiServer {
    pub async fn run_api_server(self: Arc<Self>, port: u16) -> Result<()> {
        let app = self.create_api_router().await;
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;

        info!("API server listening on port {}", port);

        axum::serve(listener, app).await.map_err(Into::into)
    }

    // Create API Router
    pub async fn create_api_router(self: Arc<Self>) -> Router {
        let state = Arc::clone(&self);
        Router::new()
            //user APIs
            // .route("/api/users", get(client::get_users))
            // .route("/api/user/tokens", get(client::get_tokens))
            // //client APIs
            .route("/api/user/insert_client", post(client::insert_client))
            .route("/api/user/client_list", get(client::get_user_clients))
            .route(
                "/api/user/client_device_detail",
                get(client::get_client_detail),
            )
            .route("/api/user/edit_client_info", post(client::edit_client_info))
            .route(
                "/api/user/client_status_list",
                get(client::get_user_client_status_list),
            )
            //client Monitoring
            .route("/api/user/client_stat", get(client::get_client_stats))
            .route("/api/user/client_monitor", get(client::get_client_monitor))
            .route("/api/user/client_health", get(client::get_client_health))
            // Model Management APIs
            .route("/api/models/insert", post(models::create_or_update_model))
            .route("/api/models/get", get(models::get_models))
            // APK Management APIs
            .route("/api/apk/upsert", post(apk::upsert_apk))
            .route("/api/apk/get", get(apk::get_apk))
            .route("/api/apk/list", get(apk::list_apk))
            .layer(CorsLayer::permissive())
            .with_state(state)
    }
}
