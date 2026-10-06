use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use sqlx::sqlite::SqlitePool;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::SizeAbove;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

/// Below this many bytes a response is sent as-is: gzipping a short JSON list
/// costs more than it saves.
const COMPRESSION_MIN_BYTES: u16 = 1024;

use crate::config::Config;
use crate::handlers::{admin, auth, comment, feed, forum, project, rating, report, trash, wiki};
use crate::services::ratelimit::{self, RateLimiter};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: Config,
    /// Failed-login budget per account. Owned per state rather than global, so
    /// tests and restarts never inherit another instance's counters.
    pub login_limiter: Arc<RateLimiter>,
    /// Sign-up budget for the whole process.
    pub register_limiter: Arc<RateLimiter>,
    /// Reports one account may file per hour.
    pub report_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: Config) -> Self {
        Self {
            pool,
            cfg,
            login_limiter: Arc::new(RateLimiter::new(
                ratelimit::LOGIN_MAX_FAILURES,
                ratelimit::LOGIN_WINDOW,
            )),
            register_limiter: Arc::new(RateLimiter::new(
                ratelimit::REGISTER_MAX_ATTEMPTS,
                ratelimit::REGISTER_WINDOW,
            )),
            report_limiter: Arc::new(RateLimiter::new(
                ratelimit::REPORT_MAX_ATTEMPTS,
                ratelimit::REPORT_WINDOW,
            )),
        }
    }
}

/// Router built from the `STATIC_DIR` environment variable.
pub fn create_router(state: AppState) -> Router {
    create_router_with_static(state, std::env::var("STATIC_DIR").ok())
}

/// Router with an explicit static directory.
///
/// `dir` is a parameter rather than an environment lookup so the static-file
/// behaviour (SPA fallback, cache headers) can be tested without mutating the
/// process environment, which is unsafe and racy across parallel tests.
pub fn create_router_with_static(state: AppState, dir: Option<String>) -> Router {
    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/refresh", post(auth::refresh))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/password", post(auth::change_password))
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
        .route(
            "/api/forum/posts",
            get(forum::list_posts).post(forum::create_post),
        )
        .route(
            "/api/forum/posts/{id}",
            get(forum::get_post)
                .patch(forum::update_post)
                .delete(forum::delete_post),
        )
        .route("/api/forum/posts/{id}/like", post(forum::like_post))
        .route(
            "/api/forum/posts/{id}/comments",
            get(forum::list_comments).post(forum::create_comment),
        )
        .route(
            "/api/forum/comments/{id}",
            axum::routing::delete(forum::delete_comment),
        )
        .route("/api/forum/comments/{id}/like", post(forum::like_comment))
        .route(
            "/api/forum/posts/{id}/featured",
            axum::routing::patch(forum::set_featured),
        )
        .route("/api/forum/rules", get(forum::rules))
        .route(
            "/api/forum/rules/{board}",
            axum::routing::put(forum::upsert_rule),
        )
        // Reports.
        .route("/api/reports", get(report::list).post(report::create))
        .route("/api/reports/mine", get(report::mine))
        .route("/api/reports/{id}", axum::routing::patch(report::resolve))
        // Feeds (public, unauthenticated by design: a reader sends no auth header).
        .route("/feed.xml", get(feed::site))
        .route("/forum/feed.xml", get(feed::forum))
        .route("/wiki/feed.xml", get(feed::wiki))
        .route("/projects/feed.xml", get(feed::projects))
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
        .route("/api/admin/audit", get(admin::list_audit));

    // Serve the built SPA when a static directory is available. Unknown paths
    // fall back to index.html so client-side routes survive a refresh.
    if let Some(dir) = dir {
        let index = std::path::Path::new(&dir).join("index.html");
        if index.exists() {
            // The Vite build writes `.br`/`.gz` siblings for every compressible
            // artifact. Without `precompressed_*` these calls a plain `ServeDir`
            // ignores the siblings entirely: they were shipped inside the image
            // and never served, while the client received uncompressed bytes.
            let static_files = ServeDir::new(&dir)
                .precompressed_br()
                .precompressed_gzip()
                .fallback(
                    ServeFile::new(&index)
                        .precompressed_br()
                        .precompressed_gzip(),
                );
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

    // Layers are applied *after* the fallback is registered on purpose:
    // `Router::layer` only wraps what already exists, so applying them earlier
    // left every static response without security headers, without the
    // `/assets/*` cache header and outside the concurrency limit.
    router
        .layer(tower::limit::ConcurrencyLimitLayer::new(64))
        // Compress API responses on the fly — wiki pages and project READMEs are
        // the payloads worth compressing. Static files already arrive encoded from
        // their `.br`/`.gz` siblings, and the layer leaves anything that carries a
        // `Content-Encoding` alone, so nothing is compressed twice. The size
        // predicate keeps small JSON lists from paying for a gzip round trip.
        .layer(CompressionLayer::new().compress_when(SizeAbove::new(COMPRESSION_MIN_BYTES)))
        .layer(axum::middleware::from_fn(
            crate::middleware::security::security_headers,
        ))
        // Outermost of the three: the recorded latency should include the wait for
        // a concurrency slot, and static responses must show up in the log too.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::extract::Request| {
                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        path = %request.uri().path(),
                    )
                })
                // `DefaultOnResponse` logs at DEBUG, which the documented
                // `RUST_LOG=…=info` filter drops — a default deployment produced no
                // access log at all. Pin the completed-request event to INFO.
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

/// Liveness/readiness probe.
///
/// It touches the database on purpose: a process that is listening but cannot
/// reach SQLite answers 200 otherwise, and the orchestrator keeps sending it
/// traffic.
async fn health(State(state): State<AppState>) -> axum::response::Response {
    match sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => {
            tracing::error!(error = %err, "health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "database unavailable").into_response()
        }
    }
}
