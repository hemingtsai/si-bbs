#![allow(dead_code)]

use axum_test::TestServer;
use si_bbs_backend::config::Config;
use si_bbs_backend::create_router;
use si_bbs_backend::routes::AppState;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

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

pub async fn test_server() -> TestServer {
    let pool = test_pool().await;
    let cfg = Config {
        database_url: "sqlite::memory:".into(),
        jwt_secret: "test-secret".into(),
        access_ttl_secs: 900,
        refresh_ttl_secs: 7 * 24 * 3600,
    };
    let app = create_router(AppState { pool, cfg });
    TestServer::new(app)
}