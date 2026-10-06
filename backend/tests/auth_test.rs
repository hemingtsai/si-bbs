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
    res.assert_status(axum::http::StatusCode::CREATED);

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
        .assert_status(axum::http::StatusCode::CREATED);

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
    app.post("/api/auth/register")
        .json(&body)
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    let res = app.post("/api/auth/register").json(&body).await;
    res.assert_status_conflict();
}

#[tokio::test]
async fn register_rejects_names_differing_only_by_case() {
    let app = common::test_server().await;
    app.post("/api/auth/register")
        .json(&json!({"username": "Alice", "email": "Alice@Example.com", "password": "secret123"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Same name, different case: `users.username` is a plain UNIQUE, so without an
    // explicit case-insensitive test this created a second, impersonating account.
    app.post("/api/auth/register")
        .json(&json!({"username": "alice", "email": "other@example.com", "password": "secret123"}))
        .await
        .assert_status_conflict();

    // Same email, different case.
    app.post("/api/auth/register")
        .json(
            &json!({"username": "someone", "email": "alice@example.com", "password": "secret123"}),
        )
        .await
        .assert_status_conflict();

    // The original account still logs in under its own spelling.
    app.post("/api/auth/login")
        .json(&json!({"username": "Alice", "password": "secret123"}))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn register_rejects_malformed_identity_and_oversized_password() {
    let app = common::test_server().await;

    let cases = [
        // Username too short / too long / illegal characters.
        json!({"username": "ab", "email": "a@b.co", "password": "secret123"}),
        json!({"username": "a".repeat(33), "email": "a@b.co", "password": "secret123"}),
        json!({"username": "a b", "email": "a@b.co", "password": "secret123"}),
        json!({"username": "ok-name", "email": "not-an-email", "password": "secret123"}),
        json!({"username": "ok-name", "email": "a@b", "password": "secret123"}),
        // 129 characters: the old byte-based check accepted unbounded input, which
        // let an unauthenticated caller hand Argon2 arbitrary work.
        json!({"username": "ok-name", "email": "a@b.co", "password": "p".repeat(129)}),
    ];
    for body in cases {
        let res = app.post("/api/auth/register").json(&body).await;
        res.assert_status(axum::http::StatusCode::BAD_REQUEST);
    }

    // 128 characters is still allowed, and a short-but-multibyte password counts
    // in characters (six CJK characters are eighteen bytes).
    app.post("/api/auth/register")
        .json(&json!({"username": "边界用户", "email": "edge@example.com", "password": "密码密码密码"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    app.post("/api/auth/login")
        .json(&json!({"username": "边界用户", "password": "密码密码密码"}))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn repeated_login_failures_are_rate_limited() {
    let app = common::test_server().await;
    common::register(&app, "dave").await;

    let attempts = si_bbs_backend::services::ratelimit::LOGIN_MAX_FAILURES;
    for i in 0..attempts {
        let res = app
            .post("/api/auth/login")
            .json(&json!({"username": "dave", "password": "wrongpass"}))
            .await;
        assert_eq!(
            res.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "attempt {i} should still be checked, not throttled"
        );
    }

    // One failure over budget: refused before the password is even looked at.
    let res = app
        .post("/api/auth/login")
        .json(&json!({"username": "dave", "password": "wrongpass"}))
        .await;
    assert_eq!(res.status_code(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    assert!(
        res.headers().get("retry-after").is_some(),
        "429 must advertise Retry-After"
    );

    // The correct password is refused too — the budget is not a password oracle.
    app.post("/api/auth/login")
        .json(&json!({"username": "dave", "password": "password123"}))
        .await
        .assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);

    // Case folding keeps `DAVE` on the same budget instead of a fresh one.
    app.post("/api/auth/login")
        .json(&json!({"username": "DAVE", "password": "password123"}))
        .await
        .assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);

    // A different account is unaffected.
    common::register(&app, "erin").await;
    common::login(&app, "erin").await;
}

#[tokio::test]
async fn a_successful_login_clears_the_failure_budget() {
    let app = common::test_server().await;
    common::register(&app, "frank").await;

    for _ in 0..(si_bbs_backend::services::ratelimit::LOGIN_MAX_FAILURES - 1) {
        app.post("/api/auth/login")
            .json(&json!({"username": "frank", "password": "wrongpass"}))
            .await
            .assert_status_unauthorized();
    }

    common::login(&app, "frank").await;

    // Back to a full budget, so the account is not locked out by a typo storm.
    for _ in 0..(si_bbs_backend::services::ratelimit::LOGIN_MAX_FAILURES - 1) {
        app.post("/api/auth/login")
            .json(&json!({"username": "frank", "password": "wrongpass"}))
            .await
            .assert_status_unauthorized();
    }
}

#[tokio::test]
async fn login_never_reveals_whether_an_account_exists() {
    let app = common::test_server().await;

    // Unknown accounts are counted against their own key and answer 401, exactly
    // like a wrong password.
    for _ in 0..si_bbs_backend::services::ratelimit::LOGIN_MAX_FAILURES {
        app.post("/api/auth/login")
            .json(&json!({"username": "ghost", "password": "whatever"}))
            .await
            .assert_status_unauthorized();
    }
    app.post("/api/auth/login")
        .json(&json!({"username": "ghost", "password": "whatever"}))
        .await
        .assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);
}

/// A password change has to end the sessions that existed before it, or a stolen
/// token survives exactly the action taken because it was stolen.
#[tokio::test]
async fn changing_the_password_invalidates_older_tokens() {
    let app = common::test_server().await;
    common::register(&app, "grace").await;
    let stale = common::login(&app, "grace").await;

    // A second session, e.g. the laptop that is about to change the password.
    let current = common::login(&app, "grace").await;

    let res = app
        .post("/api/auth/password")
        .add_header("Authorization", format!("Bearer {current}"))
        .json(&json!({"current_password": "password123", "new_password": "brand-new-pass"}))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    let fresh = body["access_token"].as_str().unwrap().to_string();

    // The session that made the change keeps working…
    app.get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {fresh}"))
        .await
        .assert_status_ok();
    // …while the other one is now refused.
    app.get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {stale}"))
        .await
        .assert_status_unauthorized();
    app.get("/api/auth/me")
        .add_header("Authorization", format!("Bearer {current}"))
        .await
        .assert_status_unauthorized();

    // The old password no longer logs in, the new one does. (`common::login` is
    // hard-coded to the registration password, so ask directly here.)
    app.post("/api/auth/login")
        .json(&json!({"username": "grace", "password": "password123"}))
        .await
        .assert_status_unauthorized();
    app.post("/api/auth/login")
        .json(&json!({"username": "grace", "password": "brand-new-pass"}))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn a_stale_refresh_token_cannot_outlive_a_password_change() {
    let app = common::test_server().await;
    common::register(&app, "heidi").await;

    let res = app
        .post("/api/auth/login")
        .json(&json!({"username": "heidi", "password": "password123"}))
        .await;
    let body = res.json::<serde_json::Value>();
    let refresh = body["refresh_token"].as_str().unwrap().to_string();
    let access = body["access_token"].as_str().unwrap().to_string();

    app.post("/api/auth/password")
        .add_header("Authorization", format!("Bearer {access}"))
        .json(&json!({"current_password": "password123", "new_password": "another-good-one"}))
        .await
        .assert_status_ok();

    // Refreshing with the pre-change token must not mint a working session.
    app.post("/api/auth/refresh")
        .json(&json!({"refresh_token": refresh}))
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn password_change_validates_input_and_the_current_password() {
    let app = common::test_server().await;
    common::register(&app, "ivan").await;
    let token = common::login(&app, "ivan").await;
    let auth = format!("Bearer {token}");

    // Wrong current password.
    app.post("/api/auth/password")
        .add_header("Authorization", auth.clone())
        .json(&json!({"current_password": "not-it", "new_password": "brand-new-pass"}))
        .await
        .assert_status_unauthorized();
    // Too short, and unchanged.
    app.post("/api/auth/password")
        .add_header("Authorization", auth.clone())
        .json(&json!({"current_password": "password123", "new_password": "short"}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    app.post("/api/auth/password")
        .add_header("Authorization", auth.clone())
        .json(&json!({"current_password": "password123", "new_password": "password123"}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    // Not logged in.
    app.post("/api/auth/password")
        .json(&json!({"current_password": "password123", "new_password": "brand-new-pass"}))
        .await
        .assert_status_unauthorized();
}
