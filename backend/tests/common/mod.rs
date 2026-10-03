#![allow(dead_code)]

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

/// Build an isolated in-memory SQLite pool with all migrations applied.
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
