//! Human verification: the challenge endpoint, and the four surfaces that demand it.
//!
//! These run with `pow_required = true`. The rest of the suite runs with it off — the
//! point of those tests is something else, and solving a challenge in each of ~250
//! tests would only add noise and time. The protocol itself is therefore covered here
//! (and end-to-end in the browser suite, where the real frontend solver runs).

mod common;

use axum::http::StatusCode;
use common::{register_and_login, test_ctx};
use jsonwebtoken::{DecodingKey, Validation, decode};
use si_bbs_backend::config::Config;
use si_bbs_backend::services::pow;

/// The secret `common::test_config` installs.
const TEST_SECRET: &str = "test-secret";

/// Read the nonce out of a challenge token, the way the browser would after asking
/// the server for one.
fn nonce_of(token: &str) -> String {
    let data = decode::<serde_json::Value>(
        token,
        &DecodingKey::from_secret(TEST_SECRET.as_bytes()),
        &Validation::default(),
    )
    .expect("the challenge should be a token this deployment signed");
    data.claims["nonce"]
        .as_str()
        .expect("challenge token carries a nonce")
        .to_string()
}

/// Ask for a challenge and solve it, returning the `pow` field a request should carry.
async fn solve_one(server: &axum_test::TestServer) -> serde_json::Value {
    let res = server.get("/api/auth/challenge").await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    let token = body["challenge"]
        .as_str()
        .expect("challenge token")
        .to_string();
    let difficulty = body["difficulty"].as_u64().expect("difficulty") as u32;
    let answer = pow::solve(&nonce_of(&token), difficulty).expect("solvable");
    serde_json::json!({ "challenge": token, "answer": answer })
}

/// A server whose config demands proof of work.
async fn pow_server() -> (axum_test::TestServer, sqlx::SqlitePool) {
    let pool = common::test_pool().await;
    let mut cfg = common::test_config("https://api.github.com");
    cfg.pow_required = true;
    // Low difficulty: these tests are about the protocol, not about CPU time.
    cfg.pow_difficulty = 8;
    (common::server_with_config(pool.clone(), cfg), pool)
}

async fn register(
    server: &axum_test::TestServer,
    username: &str,
    pow: Option<serde_json::Value>,
) -> axum_test::TestResponse {
    server
        .post("/api/auth/register")
        .json(&serde_json::json!({
            "username": username,
            "email": format!("{username}@example.com"),
            "password": "password123",
            "pow": pow,
        }))
        .await
}

#[tokio::test]
async fn the_challenge_endpoint_hands_out_something_solvable() {
    let (server, _pool) = pow_server().await;
    let res = server.get("/api/auth/challenge").await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();

    assert_eq!(body["required"], true);
    assert_eq!(body["difficulty"], 8);
    assert!(body["expires_in_secs"].as_i64().unwrap() > 0);
    let token = body["challenge"].as_str().unwrap();
    assert!(token.split('.').count() == 3, "expected a JWT: {token}");

    // Two challenges must not share a nonce, or one solution would work twice.
    let other = server
        .get("/api/auth/challenge")
        .await
        .json::<serde_json::Value>();
    assert_ne!(
        nonce_of(token),
        nonce_of(other["challenge"].as_str().unwrap())
    );
}

#[tokio::test]
async fn registration_needs_a_solved_challenge() {
    let (server, _pool) = pow_server().await;

    // No solution at all.
    let res = register(&server, "alice", None).await;
    res.assert_status(StatusCode::BAD_REQUEST);
    let body = res.json::<serde_json::Value>();
    assert_eq!(
        body["code"], "pow",
        "the frontend keys off this code: {body}"
    );

    // A solution for something the server never signed.
    let forged = register(
        &server,
        "alice",
        Some(serde_json::json!({ "challenge": "a.b.c", "answer": "0" })),
    )
    .await;
    forged.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(forged.json::<serde_json::Value>()["code"], "pow");

    // A real one works.
    let ok = register(&server, "alice", Some(solve_one(&server).await)).await;
    ok.assert_status(StatusCode::CREATED);

    // And nothing was created by the refused attempts.
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&_pool)
        .await
        .unwrap();
    assert_eq!(users, 1);
}

#[tokio::test]
async fn a_solution_is_single_use() {
    let (server, _pool) = pow_server().await;
    let solution = solve_one(&server).await;

    register(&server, "bob", Some(solution.clone()))
        .await
        .assert_status(StatusCode::CREATED);

    // The same solved challenge buys exactly one request.
    let replay = register(&server, "carol", Some(solution)).await;
    replay.assert_status(StatusCode::BAD_REQUEST);
    let body = replay.json::<serde_json::Value>();
    assert_eq!(body["code"], "pow");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("already been used"),
        "{body}"
    );
}

#[tokio::test]
async fn an_access_token_cannot_be_used_as_a_challenge() {
    let (server, _pool) = pow_server().await;
    // Register with work, then reuse the resulting access token as a "challenge".
    register(&server, "dave", Some(solve_one(&server).await))
        .await
        .assert_status(StatusCode::CREATED);
    let token = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "dave", "password": "password123" }))
        .await
        .json::<serde_json::Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_string();

    let res = register(
        &server,
        "erin",
        Some(serde_json::json!({ "challenge": token, "answer": "0" })),
    )
    .await;
    res.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(res.json::<serde_json::Value>()["code"], "pow");
}

/// An honest login stays a single request; the challenge only appears once the
/// account looks like it is being guessed at.
#[tokio::test]
async fn login_asks_for_work_only_after_repeated_failures() {
    let (server, _pool) = pow_server().await;
    register(&server, "frank", Some(solve_one(&server).await))
        .await
        .assert_status(StatusCode::CREATED);

    // First login: no challenge required.
    server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "frank", "password": "password123" }))
        .await
        .assert_status_ok();

    // A correct login does not reset the budget below the threshold, but a wrong one
    // spends it. Three failures is where the challenge kicks in.
    for _ in 0..pow::LOGIN_CHALLENGE_AFTER {
        server
            .post("/api/auth/login")
            .json(&serde_json::json!({ "username": "frank", "password": "wrong-password" }))
            .await
            .assert_status(StatusCode::UNAUTHORIZED);
    }

    let refused = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "frank", "password": "password123" }))
        .await;
    refused.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(refused.json::<serde_json::Value>()["code"], "pow");

    // With a solved challenge the correct password still works — a locked-out user is
    // not locked out, only slowed down.
    server
        .post("/api/auth/login")
        .json(&serde_json::json!({
            "username": "frank",
            "password": "password123",
            "pow": solve_one(&server).await,
        }))
        .await
        .assert_status_ok();

    // And a successful login clears the budget, so the challenge goes away again.
    server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "frank", "password": "password123" }))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn posting_and_commenting_require_work_but_editing_does_not() {
    let (server, _pool) = pow_server().await;
    register(&server, "grace", Some(solve_one(&server).await))
        .await
        .assert_status(StatusCode::CREATED);
    let token = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "username": "grace", "password": "password123" }))
        .await
        .json::<serde_json::Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    let auth = format!("Bearer {token}");

    // A post without work is refused.
    let refused = server
        .post("/api/forum/posts")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({ "board": "life", "title": "hi", "content": "body" }))
        .await;
    refused.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(refused.json::<serde_json::Value>()["code"], "pow");

    // With work it is created…
    let post = server
        .post("/api/forum/posts")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "board": "life", "title": "hi", "content": "body",
            "pow": solve_one(&server).await,
        }))
        .await;
    post.assert_status(StatusCode::CREATED);
    let id = post.json::<serde_json::Value>()["id"].as_i64().unwrap();

    // …a reply needs its own solution…
    server
        .post(&format!("/api/forum/posts/{id}/comments"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({ "content": "reply" }))
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    server
        .post(&format!("/api/forum/posts/{id}/comments"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "content": "reply",
            "pow": solve_one(&server).await,
        }))
        .await
        .assert_status(StatusCode::CREATED);

    // …but editing your own post does not: it is not a spam surface, and demanding
    // work there would punish the author for fixing a typo.
    server
        .patch(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({ "board": "life", "title": "fixed typo", "content": "body" }))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn requesting_a_password_reset_requires_work() {
    let (server, _pool) = pow_server().await;
    register(&server, "heidi", Some(solve_one(&server).await))
        .await
        .assert_status(StatusCode::CREATED);

    let refused = server
        .post("/api/auth/forgot")
        .json(&serde_json::json!({ "email": "heidi@example.com" }))
        .await;
    refused.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(refused.json::<serde_json::Value>()["code"], "pow");

    server
        .post("/api/auth/forgot")
        .json(&serde_json::json!({
            "email": "heidi@example.com",
            "pow": solve_one(&server).await,
        }))
        .await
        .assert_status(StatusCode::ACCEPTED);
}

#[tokio::test]
async fn an_unsolved_answer_is_refused_and_the_challenge_survives() {
    let (server, _pool) = pow_server().await;
    let res = server.get("/api/auth/challenge").await;
    let token = res.json::<serde_json::Value>()["challenge"]
        .as_str()
        .unwrap()
        .to_string();

    // A wrong answer is rejected…
    let wrong = register(
        &server,
        "ivan",
        Some(serde_json::json!({ "challenge": token.clone(), "answer": "definitely-not" })),
    )
    .await;
    wrong.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(wrong.json::<serde_json::Value>()["code"], "pow");

    // …and does not consume the challenge, so an honest client can still submit the
    // answer it is working on.
    let answer = pow::solve(&nonce_of(&token), 8).unwrap();
    register(
        &server,
        "ivan",
        Some(serde_json::json!({ "challenge": token, "answer": answer })),
    )
    .await
    .assert_status(StatusCode::CREATED);
}

/// With the challenge switched off the same requests go through untouched, and the
/// endpoint says so — one code path in the frontend either way.
#[tokio::test]
async fn nothing_is_demanded_when_the_deployment_disables_it() {
    let (server, _pool) = test_ctx().await;

    let body = server
        .get("/api/auth/challenge")
        .await
        .json::<serde_json::Value>();
    assert_eq!(body["required"], false);
    assert_eq!(body["difficulty"], 0);

    let token = register_and_login(&server, "judy").await;
    assert!(!token.is_empty());
    // Posting works with no `pow` field at all.
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "board": "life", "title": "no pow", "content": "body" }))
        .await
        .assert_status(StatusCode::CREATED);
}

/// The deployment's own difficulty is what gets handed out, clamped to something a
/// browser could actually solve.
#[tokio::test]
async fn the_configured_difficulty_is_what_is_handed_out() {
    let pool = common::test_pool().await;
    let mut cfg: Config = common::test_config("https://api.github.com");
    cfg.pow_required = true;
    cfg.pow_difficulty = pow::MAX_DIFFICULTY + 5;
    let server = common::server_with_config(pool, cfg);

    let body = server
        .get("/api/auth/challenge")
        .await
        .json::<serde_json::Value>();
    assert_eq!(body["difficulty"], pow::MAX_DIFFICULTY);
}
