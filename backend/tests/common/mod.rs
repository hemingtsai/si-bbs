#![allow(dead_code)]

use axum_test::TestServer;
use si_bbs_backend::config::Config;
use si_bbs_backend::create_router;
use si_bbs_backend::routes::{AppState, create_router_with_static};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

pub async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

/// Router plus the pool, so tests can seed users or flip timestamps.
pub async fn test_ctx() -> (TestServer, SqlitePool) {
    test_ctx_with_github("https://api.github.com").await
}

/// Same as [`test_ctx`] but pointing the GitHub client at a mock server.
pub async fn test_ctx_with_github(github_api_base: &str) -> (TestServer, SqlitePool) {
    let pool = test_pool().await;
    (server_with(pool.clone(), github_api_base), pool)
}

/// Build a router over an existing pool. Useful to swap the GitHub base
/// (wiremock keeps the first mounted mock, so a second mock server is the way
/// to change GitHub's answer mid-test).
pub fn server_with(pool: SqlitePool, github_api_base: &str) -> TestServer {
    TestServer::new(create_router(AppState::new(
        pool,
        test_config(github_api_base),
    )))
}

/// Router that serves a static directory as well as the API, so the SPA
/// fallback and its response headers can be exercised without touching the
/// process environment.
pub fn server_with_static(pool: SqlitePool, static_dir: Option<String>) -> TestServer {
    let state = AppState::new(pool, test_config("https://api.github.com"));
    TestServer::new(create_router_with_static(state, static_dir))
}

fn test_config(github_api_base: &str) -> Config {
    Config {
        database_url: "sqlite::memory:".into(),
        jwt_secret: "test-secret".into(),
        access_ttl_secs: 900,
        refresh_ttl_secs: 7 * 24 * 3600,
        github_token: String::new(),
        github_api_base: github_api_base.to_string(),
    }
}

pub async fn test_server() -> TestServer {
    test_ctx().await.0
}

/// Create a user. Returns 409 if the username is taken.
pub async fn register(server: &TestServer, username: &str) {
    server
        .post("/api/auth/register")
        .json(&serde_json::json!({
            "username": username,
            "email": format!("{username}@example.com"),
            "password": "password123",
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// Log in and return the access token.
pub async fn login(server: &TestServer, username: &str) -> String {
    let res = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": username, "password": "password123" }))
        .await;
    res.assert_status_ok();
    res.json::<serde_json::Value>()["access_token"]
        .as_str()
        .expect("access_token")
        .to_string()
}

/// Register a user and return its login access token.
pub async fn register_and_login(server: &TestServer, username: &str) -> String {
    register(server, username).await;
    login(server, username).await
}

/// Register a user, promote it to the given role, then log in so the token
/// carries the new role.
pub async fn register_login_as_role(
    server: &TestServer,
    pool: &SqlitePool,
    username: &str,
    role: &str,
) -> String {
    register(server, username).await;
    sqlx::query("UPDATE users SET role = ?2 WHERE username = ?1")
        .bind(username)
        .bind(role)
        .execute(pool)
        .await
        .expect("promote user");
    login(server, username).await
}
