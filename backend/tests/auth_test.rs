mod common;

use serde_json::json;

#[tokio::test]
async fn register_login_me_refresh_happy_path() {
    let app = common::test_server().await;

    // AUTH-01: register
    let res = app
        .post("/api/auth/register")
        .json(&json!({"username": "alice", "email": "alice@example.com", "password": "secret123"}))
        .await;
    res.assert_status_ok();

    // AUTH-01: login -> JWT
    let res = app
        .post("/api/auth/login")
        .json(&json!({"username": "alice", "password": "secret123"}))
        .await;
    res.assert_status_ok();
    let body: serde_json::Value = res.json();
    let access = body["access_token"].as_str().unwrap();
    let refresh = body["refresh_token"].as_str().unwrap();
    assert!(!access.is_empty() && !refresh.is_empty());

    // AUTH-01: /me with bearer
    let res = app
        .get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {access}"))
        .await;
    res.assert_status_ok();
    let me: serde_json::Value = res.json();
    assert_eq!(me["username"], "alice");
    assert_eq!(me["role"], "user");

    // AUTH-04: refresh -> new tokens
    let res = app
        .post("/api/auth/refresh")
        .json(&json!({"refresh_token": refresh}))
        .await;
    res.assert_status_ok();
}

#[tokio::test]
async fn wrong_password_returns_401() {
    let app = common::test_server().await;
    app.post("/api/auth/register")
        .json(&json!({"username": "bob", "email": "bob@example.com", "password": "secret123"}))
        .await
        .assert_status_ok();

    let res = app
        .post("/api/auth/login")
        .json(&json!({"username": "bob", "password": "wrongpass"}))
        .await;
    // AUTH-02
    res.assert_status_unauthorized();
}

#[tokio::test]
async fn unauthenticated_me_returns_401() {
    let app = common::test_server().await;
    let res = app.get("/api/auth/me").await;
    // AUTH-03 equivalent: no token
    res.assert_status_unauthorized();
}

#[tokio::test]
async fn duplicate_register_returns_409() {
    let app = common::test_server().await;
    let body = json!({"username": "carol", "email": "carol@example.com", "password": "secret123"});
    app.post("/api/auth/register").json(&body).await.assert_status_ok();
    let res = app.post("/api/auth/register").json(&body).await;
    res.assert_status_conflict();
}
