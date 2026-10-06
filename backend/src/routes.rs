use axum::Router;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use sqlx::sqlite::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};

use crate::config::Config;
use crate::handlers::{admin, auth, comment, forum, project, rating, trash, wiki};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: Config,
}

pub fn create_router(state: AppState) -> Router {
    let mut router = Router::new()
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
        .route("/api/forum/boards", get(forum::boards))
        .route("/api/forum/posts", get(forum::list_posts).post(forum::create_post))
        .route("/api/forum/posts/{id}", get(forum::get_post).patch(forum::update_post).delete(forum::delete_post))
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
        .layer(tower::limit::ConcurrencyLimitLayer::new(64));

    // Serve the built SPA when a static directory is available. Unknown paths
    // fall back to index.html so client-side routes survive a refresh.
    if let Ok(dir) = std::env::var("STATIC_DIR") {
        let index = std::path::Path::new(&dir).join("index.html");
        if index.exists() {
            let static_files = ServeDir::new(&dir).fallback(ServeFile::new(&index));
            router = router.fallback(move |req: axum::extract::Request| {
                let mut service = static_files.clone();
                async move {
                    // Unknown API paths must keep returning a JSON 404 instead of
                    // the SPA shell.
                    if req.uri().path().starts_with("/api/") {
                        return crate::error::AppError::NotFound.into_response();
                    }
                    match tower::ServiceExt::oneshot(&mut service, req).await {
                        Ok(response) => response.into_response(),
                        Err(_) => {
                            crate::error::AppError::Internal("static file".into()).into_response()
                        }
                    }
                }
            });
        }
    }

    router.with_state(state)
}
