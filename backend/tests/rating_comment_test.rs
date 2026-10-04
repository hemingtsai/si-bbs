mod common;

use axum_test::TestServer;
use common::{register_and_login, test_ctx};
use si_bbs_backend::services::github::parse_repo_url;
use sqlx::SqlitePool;

/// Insert an approved project directly so rating/comment tests do not depend on
/// the GitHub integration.
async fn seed_approved_project(pool: &SqlitePool, username: &str) -> i64 {
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?1")
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap();
    let res = sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, category, status, submitted_by) \
         VALUES ('demo', 'https://github.com/o/demo', 'o', 'demo', 'dev-tools', 'approved', ?1)",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
    res.last_insert_rowid()
}

async fn seed_pending_project(pool: &SqlitePool, username: &str) -> i64 {
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?1")
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap();
    let res = sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, category, status, submitted_by) \
         VALUES ('pending-demo', 'https://github.com/o/pending', 'o', 'pending', 'dev-tools', \
         'pending', ?1)",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
    res.last_insert_rowid()
}

async fn rate(
    server: &TestServer,
    token: &str,
    project_id: i64,
    score: i64,
    comment: &str,
) -> axum_test::TestResponse {
    server
        .post(&format!("/api/projects/{project_id}/rating"))
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "score": score, "comment": comment }))
        .await
}

#[tokio::test]
async fn rating_creates_then_updates_one_row_per_user() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    let res = rate(&server, &alice, project_id, 8, "solid").await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["average"], 8.0);
    assert_eq!(body["count"], 1);
    assert_eq!(body["my_score"], 8);

    // Rating again updates instead of inserting a second row.
    let res = rate(&server, &alice, project_id, 10, "amazing").await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["average"], 10.0);
    assert_eq!(body["count"], 1);

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ratings WHERE project_id = ?1")
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn rating_average_aggregates_across_users() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let carol = register_and_login(&server, "carol").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    rate(&server, &alice, project_id, 9, "")
        .await
        .assert_status_ok();
    rate(&server, &bob, project_id, 7, "")
        .await
        .assert_status_ok();
    rate(&server, &carol, project_id, 8, "")
        .await
        .assert_status_ok();

    let res = server
        .get(&format!("/api/projects/{project_id}/rating/summary"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["count"], 3);
    assert_eq!(body["average"], 8.0);
    assert_eq!(body["project_id"], project_id);
}

#[tokio::test]
async fn rating_rejects_out_of_range_scores() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    for score in [0, -1, 11, 100] {
        rate(&server, &alice, project_id, score, "")
            .await
            .assert_status(axum::http::StatusCode::BAD_REQUEST);
    }

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ratings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn rating_requires_login_and_known_project() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    server
        .post(&format!("/api/projects/{project_id}/rating"))
        .json(&serde_json::json!({ "score": 5 }))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);

    rate(&server, &alice, 999_999, 5, "")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    server
        .get("/api/projects/999999/rating/summary")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn pending_project_cannot_be_rated() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_pending_project(&pool, "alice").await;

    // Not visible yet, so rating it 404s rather than leaking its existence.
    rate(&server, &alice, project_id, 5, "")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    server
        .get(&format!("/api/projects/{project_id}/rating/summary"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rating_summary_is_public_and_zero_when_unrated() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    let res = server
        .get(&format!("/api/projects/{project_id}/rating/summary"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["count"], 0);
    assert_eq!(body["average"], 0.0);

    rate(&server, &alice, project_id, 6, "")
        .await
        .assert_status_ok();
}

async fn comment(
    server: &TestServer,
    token: &str,
    project_id: i64,
    content: &str,
) -> axum_test::TestResponse {
    server
        .post(&format!("/api/projects/{project_id}/comments"))
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "content": content }))
        .await
}

#[tokio::test]
async fn comment_is_created_with_author_username() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    let res = comment(&server, &alice, project_id, "  works great  ").await;
    res.assert_status(axum::http::StatusCode::CREATED);
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["username"], "alice");
    assert_eq!(body["content"], "works great");
    assert_eq!(body["project_id"], project_id);
}

#[tokio::test]
async fn comment_requires_login_and_non_empty_content() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    server
        .post(&format!("/api/projects/{project_id}/comments"))
        .json(&serde_json::json!({ "content": "hi" }))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);

    for bad in ["", "   ", "\n\t "] {
        comment(&server, &alice, project_id, bad)
            .await
            .assert_status(axum::http::StatusCode::BAD_REQUEST);
    }

    let long = "x".repeat(5001);
    comment(&server, &alice, project_id, &long)
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn comment_list_is_public_ordered_and_paginated() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    comment(&server, &alice, project_id, "first")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    comment(&server, &bob, project_id, "second")
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let res = server
        .get(&format!("/api/projects/{project_id}/comments"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 2);
    assert_eq!(body["items"][0]["content"], "first");
    assert_eq!(body["items"][1]["username"], "bob");

    let res = server
        .get(&format!(
            "/api/projects/{project_id}/comments?per_page=1&page=2"
        ))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["content"], "second");
    assert_eq!(body["total"], 2);
}

#[tokio::test]
async fn soft_deleted_comments_disappear_from_list() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_approved_project(&pool, "alice").await;

    comment(&server, &alice, project_id, "keep me")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    comment(&server, &alice, project_id, "hide me")
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Soft delete directly; the DELETE endpoints arrive with the trash phase.
    sqlx::query("UPDATE comments SET deleted_at = CURRENT_TIMESTAMP WHERE content = 'hide me'")
        .execute(&pool)
        .await
        .unwrap();

    let res = server
        .get(&format!("/api/projects/{project_id}/comments"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["content"], "keep me");
}

#[tokio::test]
async fn pending_project_cannot_be_commented_on() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_pending_project(&pool, "alice").await;

    // Not visible yet, so commenting 404s rather than leaking its existence.
    comment(&server, &alice, project_id, "nice")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    server
        .get(&format!("/api/projects/{project_id}/comments"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

/// Guard against the URL parser regressing while other suites pass.
#[test]
fn github_url_parser_still_rejects_bare_owner() {
    assert!(parse_repo_url("https://github.com/owner").is_err());
}
