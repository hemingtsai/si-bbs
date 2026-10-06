//! The probe has to touch the database: a process that answers 200 while SQLite
//! is unreachable keeps receiving traffic from the orchestrator.

mod common;

use axum::http::StatusCode;

#[tokio::test]
async fn health_reports_ok_while_the_database_answers() {
    let (server, _pool) = common::test_ctx().await;
    let res = server.get("/api/health").await;
    res.assert_status_ok();
    assert_eq!(res.text(), "ok");
}

#[tokio::test]
async fn health_reports_unavailable_when_the_database_is_gone() {
    let (server, pool) = common::test_ctx().await;
    server.get("/api/health").await.assert_status_ok();

    pool.close().await;

    let res = server.get("/api/health").await;
    res.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.text(), "database unavailable");
}
