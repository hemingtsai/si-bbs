use axum::Router;
use axum::routing::{get, post};
use sqlx::sqlite::SqlitePool;

use crate::config::Config;
use crate::handlers::{admin, auth, comment, project, rating, trash, wiki};

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
        // Wiki.
        .route("/api/wiki", get(wiki::list).post(wiki::create))
        .route("/api/wiki/mine", get(wiki::mine))
        .route("/api/wiki/categories", get(wiki::categories))
        .route("/api/wiki/{slug}", get(wiki::detail))
        .route("/api/wiki/page/{id}", axum::routing::put(wiki::update))
        .route("/api/wiki/page/{id}", axum::routing::delete(wiki::delete))
        .route("/api/projects", get(project::list).post(project::submit))
        .route("/api/projects/mine", get(project::mine))
        .route("/api/projects/review-queue", get(project::review_queue))
        .route(
            "/api/projects/{id}",
            get(project::detail).delete(project::delete),
        )
        .route("/api/projects/{id}/review", post(project::review))
        .route("/api/projects/{id}/rating", post(rating::rate))
        .route("/api/projects/{id}/rating/summary", get(rating::summary))
        .route(
            "/api/projects/{id}/comments",
            get(comment::list).post(comment::create),
        )
        .route(
            "/api/projects/{project_id}/comments/{comment_id}",
            axum::routing::delete(comment::delete),
        )
        // Trash.
        .route("/api/trash", get(trash::list))
        .route("/api/trash/{kind}/{id}/restore", post(trash::restore))
        .route(
            "/api/trash/{kind}/{id}",
            axum::routing::delete(trash::purge),
        )
        // Administration.
        .route("/api/admin/users", get(admin::list_users))
        .route(
            "/api/admin/users/{id}/role",
            axum::routing::patch(admin::set_role),
        )
        .route(
            "/api/admin/users/{id}/ban",
            axum::routing::patch(admin::set_ban),
        )
        .route("/api/admin/stats", get(admin::stats))
        .layer(tower::limit::ConcurrencyLimitLayer::new(64))
        .with_state(state)
}
