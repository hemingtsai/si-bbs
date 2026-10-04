use axum::routing::{get, post};
use axum::Router;
use sqlx::sqlite::SqlitePool;

use crate::config::Config;
use crate::handlers::auth;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: Config,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/refresh", post(auth::refresh))
        .route("/api/auth/me", get(auth::me))
        .layer(tower::limit::ConcurrencyLimitLayer::new(64))
        .with_state(state)
}