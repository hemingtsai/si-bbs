mod common;

use axum_test::TestServer;
use common::{register, register_and_login, register_login_as_role, test_ctx};
use sqlx::SqlitePool;

async fn admin_server() -> (TestServer, SqlitePool) {
    let (server, pool) = test_ctx().await;
    register_login_as_role(&server, &pool, "root", "admin").await;
    (server, pool)
}

/// Log in as the `root` admin created by [`admin_server`].
async fn admin_token(server: &TestServer) -> String {
    common::login(server, "root").await
}

async fn user_id_of(server: &TestServer, username: &str) -> i64 {
    let res = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": username, "password": "password123" }))
        .await;
    res.assert_status_ok();
    res.json::<serde_json::Value>()["user_id"]
        .as_i64()
        .expect("login must expose user_id")
}

#[tokio::test]
async fn admin_lists_users_with_search_and_pagination() {
    let (server, _pool) = admin_server().await;
    let admin = admin_token(&server).await;
    for name in ["alice", "bob", "carol"] {
        register(&server, name).await;
    }

    let res = server
        .get("/api/admin/users")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 4);
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 20);

    let res = server
        .get("/api/admin/users?q=ali")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await;
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["username"], "alice");

    let res = server
        .get("/api/admin/users?per_page=2&page=2")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await;
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 4);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);

    let res = server
        .get("/api/admin/users?role=admin")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    // An unknown role filter is rejected instead of silently matching everyone.
    server
        .get("/api/admin/users?role=wizard")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_endpoints_are_closed_to_non_admins() {
    let (server, pool) = test_ctx().await;
    let user = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let admin = { register_login_as_role(&server, &pool, "root", "admin").await };

    // Anonymous.
    for path in ["/api/admin/users", "/api/admin/stats"] {
        server
            .get(path)
            .await
            .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    }

    // Plain user and moderator are both refused.
    for path in ["/api/admin/users", "/api/admin/stats"] {
        server
            .get(path)
            .add_header("Authorization", format!("Bearer {user}"))
            .await
            .assert_status(axum::http::StatusCode::FORBIDDEN);
        server
            .get(path)
            .add_header("Authorization", format!("Bearer {mod_token}"))
            .await
            .assert_status(axum::http::StatusCode::FORBIDDEN);
    }

    server
        .get("/api/admin/stats")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn stats_counts_users_projects_and_trash() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "alice").await;

    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, category, status, submitted_by) \
         VALUES ('a', 'https://github.com/o/a', 'o', 'a', 'c', 'approved', ?1)",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, category, status, submitted_by) \
         VALUES ('b', 'https://github.com/o/b', 'o', 'b', 'c', 'pending', ?1)",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE wiki_pages SET deleted_at = CURRENT_TIMESTAMP WHERE id = -1")
        .execute(&pool)
        .await
        .unwrap();

    let res = server
        .get("/api/admin/stats")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["users"], 2);
    assert_eq!(body["users_banned"], 0);
    assert_eq!(body["projects"], 2);
    assert_eq!(body["projects_approved"], 1);
    assert_eq!(body["projects_pending"], 1);
    assert_eq!(body["comments"], 0);
    assert_eq!(body["ratings"], 0);
    assert_eq!(body["trashed"], 0);
}

#[tokio::test]
async fn admin_changes_a_role_and_the_change_is_visible_immediately() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "alice").await;
    let alice = common::login(&server, "alice").await;
    let alice_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let res = server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "moderator" }))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["role"], "moderator");

    // The old token still says "user", but /me reports the live role.
    let res = server
        .get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["role"], "moderator");

    // And the promoted account can immediately use a moderator-only endpoint.
    server
        .get("/api/projects/review-queue")
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn a_demoted_admin_loses_access_on_the_next_request() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    // Promote alice to admin, let her log in, then demote her again.
    register(&server, "alice").await;
    let alice_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "admin" }))
        .await
        .assert_status_ok();
    let alice_admin = common::login(&server, "alice").await;

    server
        .get("/api/admin/stats")
        .add_header("Authorization", format!("Bearer {alice_admin}"))
        .await
        .assert_status_ok();

    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "user" }))
        .await
        .assert_status_ok();

    // Her still-valid admin token must stop working immediately.
    server
        .get("/api/admin/stats")
        .add_header("Authorization", format!("Bearer {alice_admin}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn refresh_does_not_resurrect_a_revoked_role() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "alice").await;
    let alice_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "moderator" }))
        .await
        .assert_status_ok();

    let res = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "alice", "password": "password123" }))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    let refresh = body["refresh_token"].as_str().unwrap().to_string();
    let access = body["access_token"].as_str().unwrap().to_string();

    // Demote while alice holds a live access token and refresh token.
    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "user" }))
        .await
        .assert_status_ok();

    // Refreshing must read the database, not copy the role out of the token.
    let res = server
        .post("/api/auth/refresh")
        .json(&serde_json::json!({ "refresh_token": refresh }))
        .await;
    res.assert_status_ok();
    let new_access = res.json::<serde_json::Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(new_access, access);

    let res = server
        .get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {new_access}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["role"], "user");
}

#[tokio::test]
async fn admin_validates_the_role_value() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "alice").await;
    let alice_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();

    for bad in ["wizard", "", "ADMIN", "root"] {
        server
            .patch(&format!("/api/admin/users/{alice_id}/role"))
            .add_header("Authorization", format!("Bearer {admin}"))
            .json(&serde_json::json!({ "role": bad }))
            .await
            .assert_status(axum::http::StatusCode::BAD_REQUEST);
    }

    server
        .patch("/api/admin/users/999999/role")
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "user" }))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // A non-admin cannot change roles either.
    let alice = common::login(&server, "alice").await;
    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({ "role": "admin" }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_last_admin_cannot_be_demoted() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    let root_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'root'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let res = server
        .patch(&format!("/api/admin/users/{root_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "user" }))
        .await;
    res.assert_status(axum::http::StatusCode::CONFLICT);

    // With a second admin in place the demotion is allowed.
    register(&server, "alice").await;
    let alice_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "admin" }))
        .await
        .assert_status_ok();

    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "user" }))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn admin_cannot_demote_itself_while_others_remain() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "alice").await;
    let alice_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .patch(&format!("/api/admin/users/{alice_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "admin" }))
        .await
        .assert_status_ok();

    let root_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'root'")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .patch(&format!("/api/admin/users/{root_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "user" }))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn banning_blocks_login_and_privileged_endpoints() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    let bob = register_and_login(&server, "bob").await;
    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let res = server
        .patch(&format!("/api/admin/users/{bob_id}/ban"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "banned": true }))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["banned"], 1);

    // A banned member cannot log in, even with the right password.
    let res = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "bob", "password": "password123" }))
        .await;
    res.assert_status(axum::http::StatusCode::FORBIDDEN);

    // Their existing token is dead for privileged endpoints...
    server
        .get("/api/admin/stats")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    // ...and for /me, which reports current standing.
    server
        .get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    // Unbanning restores access.
    server
        .patch(&format!("/api/admin/users/{bob_id}/ban"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "banned": false }))
        .await
        .assert_status_ok();
    server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "bob", "password": "password123" }))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn banned_user_cannot_refresh() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "bob").await;
    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let res = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "bob", "password": "password123" }))
        .await;
    let refresh = res.json::<serde_json::Value>()["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();

    server
        .patch(&format!("/api/admin/users/{bob_id}/ban"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "banned": true }))
        .await
        .assert_status_ok();

    server
        .post("/api/auth/refresh")
        .json(&serde_json::json!({ "refresh_token": refresh }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_cannot_ban_itself_or_the_last_admin() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    let root_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'root'")
        .fetch_one(&pool)
        .await
        .unwrap();

    server
        .patch(&format!("/api/admin/users/{root_id}/ban"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "banned": true }))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    server
        .patch("/api/admin/users/999999/ban")
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "banned": true }))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    register(&server, "bob").await;
    let bob = common::login(&server, "bob").await;
    server
        .patch(&format!("/api/admin/users/{root_id}/ban"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "banned": true }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn stats_counts_banned_members() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "bob").await;
    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .patch(&format!("/api/admin/users/{bob_id}/ban"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "banned": true }))
        .await
        .assert_status_ok();

    let res = server
        .get("/api/admin/stats")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await;
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["users"], 2);
    assert_eq!(body["users_banned"], 1);
}

#[tokio::test]
async fn login_response_exposes_user_id() {
    let (server, _pool) = test_ctx().await;
    register(&server, "alice").await;
    let id = user_id_of(&server, "alice").await;
    assert!(id > 0);
}

#[tokio::test]
async fn banned_user_loses_all_write_access_immediately() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "bob").await;
    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let bob = common::login(&server, "bob").await;

    // Confirm the token works before the ban.
    server
        .get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status_ok();

    server
        .patch(&format!("/api/admin/users/{bob_id}/ban"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "banned": true }))
        .await
        .assert_status_ok();

    // Every write endpoint must refuse immediately, not at token expiry.
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "board": "models", "title": "t", "content": "c" }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "title": "t", "category": "c", "content": "x" }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    server
        .post("/api/projects")
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "github_url": "https://github.com/a/b", "category": "x" }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn demoted_moderator_cannot_delete_others_post_with_old_token() {
    let (server, pool) = admin_server().await;
    let admin = admin_token(&server).await;
    register(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod1", "moderator").await;

    // mod1's token shows moderator role. Demote them to plain user.
    let mod_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'mod1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .patch(&format!("/api/admin/users/{mod_id}/role"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .json(&serde_json::json!({ "role": "user" }))
        .await
        .assert_status_ok();

    server
        .patch("/api/forum/posts/999/featured")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({ "featured": true }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
}
