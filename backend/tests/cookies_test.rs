//! Browser sessions: httpOnly cookies plus the CSRF guard for cookie-authenticated
//! writes, while `Authorization: Bearer` keeps working for the API and the rest of
//! the suite.

mod common;

use axum::http::{StatusCode, header};
use common::{register_and_login, test_ctx};

fn set_cookies(res: &axum_test::TestResponse) -> Vec<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_string)
        .collect()
}

/// Value of one cookie from the response's `Set-Cookie` headers.
fn cookie_value(res: &axum_test::TestResponse, name: &str) -> String {
    set_cookies(res)
        .into_iter()
        .find_map(|c| {
            c.strip_prefix(&format!("{name}="))
                .map(|rest| rest.split(';').next().unwrap_or("").to_string())
        })
        .unwrap_or_default()
}

async fn login_cookies(server: &axum_test::TestServer, username: &str) -> (String, String, String) {
    let res = server
        .post("/api/auth/login")
        .json(&serde_json::json!({"username": username, "password": "password123"}))
        .await;
    res.assert_status_ok();
    // The JSON pair must stay: the API and every other test use it.
    let body = res.json::<serde_json::Value>();
    assert!(body["access_token"].as_str().is_some_and(|t| !t.is_empty()));
    (
        cookie_value(&res, "access_token"),
        cookie_value(&res, "refresh_token"),
        cookie_value(&res, "csrf_token"),
    )
}

#[tokio::test]
async fn logging_in_sets_a_locked_down_session() {
    let (server, _pool) = test_ctx().await;
    common::register(&server, "cookie_user").await;
    let res = server
        .post("/api/auth/login")
        .json(&serde_json::json!({"username": "cookie_user", "password": "password123"}))
        .await;

    let cookies = set_cookies(&res);
    assert_eq!(
        cookies.len(),
        3,
        "expected access, refresh and csrf: {cookies:?}"
    );

    let access = cookies
        .iter()
        .find(|c| c.starts_with("access_token="))
        .unwrap();
    let refresh = cookies
        .iter()
        .find(|c| c.starts_with("refresh_token="))
        .unwrap();
    let csrf = cookies
        .iter()
        .find(|c| c.starts_with("csrf_token="))
        .unwrap();

    // A token JavaScript can read is a token an XSS bug can steal.
    assert!(access.contains("HttpOnly"), "{access}");
    assert!(refresh.contains("HttpOnly"), "{refresh}");
    assert!(access.contains("Path=/api;"), "{access}");
    assert!(refresh.contains("Path=/api/auth;"), "{refresh}");
    // SameSite=Lax already stops the cookie riding along on a cross-site POST.
    assert!(access.contains("SameSite=Lax"), "{access}");
    // The CSRF cookie must be readable, because the client echoes it in a header.
    assert!(!csrf.contains("HttpOnly"), "{csrf}");
    // Local tests run over http, so `Secure` would make the cookie unusable.
    assert!(!access.contains("Secure"), "{access}");
}

#[tokio::test]
async fn a_cookie_authenticates_without_any_authorization_header() {
    let (server, _pool) = test_ctx().await;
    common::register(&server, "cookie_only").await;
    let (access, _, _) = login_cookies(&server, "cookie_only").await;
    assert!(!access.is_empty());

    let res = server
        .get("/api/auth/profile")
        .add_header("Cookie", format!("access_token={access}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["username"], "cookie_only");

    // A bogus cookie is refused, not silently ignored.
    server
        .get("/api/auth/profile")
        .add_header("Cookie", "access_token=not-a-token")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    // And the header still wins when both are present: this is what keeps Bearer
    // clients immune to whatever cookies their environment holds.
    let json_token = register_and_login(&server, "header_wins").await;
    server
        .get("/api/auth/profile")
        .add_header("Authorization", format!("Bearer {json_token}"))
        .add_header("Cookie", "access_token=not-a-token")
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn cookie_writes_need_the_double_submit_token() {
    let (server, _pool) = test_ctx().await;
    common::register(&server, "csrf_user").await;
    let (access, _, csrf) = login_cookies(&server, "csrf_user").await;
    let cookie = format!("access_token={access}; csrf_token={csrf}");
    let post = serde_json::json!({"board": "life", "title": "通过 cookie 发帖", "content": "正文"});

    // No CSRF header: refused, with a code the client can act on.
    let refused = server
        .post("/api/forum/posts")
        .add_header("Cookie", cookie.clone())
        .json(&post)
        .await;
    refused.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(refused.json::<serde_json::Value>()["code"], "csrf");

    // A wrong token is just as bad as none.
    server
        .post("/api/forum/posts")
        .add_header("Cookie", cookie.clone())
        .add_header("x-csrf-token", "guessed")
        .json(&post)
        .await
        .assert_status(StatusCode::FORBIDDEN);

    // The matching token is accepted.
    server
        .post("/api/forum/posts")
        .add_header("Cookie", cookie.clone())
        .add_header("x-csrf-token", csrf.clone())
        .json(&post)
        .await
        .assert_status(StatusCode::CREATED);

    // Reading is never blocked: a GET has no side effect to forge.
    server
        .get("/api/auth/profile")
        .add_header("Cookie", cookie.clone())
        .await
        .assert_status_ok();

    // A Bearer request is exempt, because a hostile page cannot set that header.
    let token = register_and_login(&server, "csrf_bearer").await;
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&post)
        .await
        .assert_status(StatusCode::CREATED);
}

#[tokio::test]
async fn the_refresh_cookie_rotates_the_session_without_javascript() {
    let (server, _pool) = test_ctx().await;
    common::register(&server, "rotate_user").await;
    let (access, refresh, _) = login_cookies(&server, "rotate_user").await;

    // No body at all: the cookie is the whole request.
    let res = server
        .post("/api/auth/refresh")
        .add_header("Cookie", format!("refresh_token={refresh}"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert!(body["access_token"].as_str().is_some_and(|t| !t.is_empty()));
    // Rotation also reissues the cookies, so the browser keeps sliding.
    assert!(!cookie_value(&res, "access_token").is_empty());
    assert!(!cookie_value(&res, "refresh_token").is_empty());

    // An access token is not a refresh token.
    server
        .post("/api/auth/refresh")
        .add_header("Cookie", format!("refresh_token={access}"))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    // And nothing at all is a 401, not a 422.
    server
        .post("/api/auth/refresh")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logging_out_clears_every_cookie() {
    let (server, _pool) = test_ctx().await;
    common::register(&server, "bye_user").await;
    login_cookies(&server, "bye_user").await;

    let res = server.post("/api/auth/logout").await;
    res.assert_status_ok();
    let cookies = set_cookies(&res);
    assert_eq!(cookies.len(), 3);
    for cookie in &cookies {
        assert!(cookie.contains("Max-Age=0"), "{cookie}");
        // Not `Secure`: a deletion cookie over http would be dropped, and the
        // session would survive the logout.
        assert!(!cookie.contains("Secure"), "{cookie}");
    }
    assert!(cookies[0].starts_with("access_token=;"), "{:?}", cookies[0]);
}

/// Changing the password reissues the cookies along with the JSON pair, otherwise the
/// browser would keep presenting a token the server has just invalidated.
#[tokio::test]
async fn changing_the_password_reissues_the_cookies() {
    let (server, _pool) = test_ctx().await;
    common::register(&server, "reissue_user").await;
    let (access, _, csrf) = login_cookies(&server, "reissue_user").await;

    let res = server
        .post("/api/auth/password")
        .add_header(
            "Cookie",
            format!("access_token={access}; csrf_token={csrf}"),
        )
        .add_header("x-csrf-token", csrf.clone())
        .json(&serde_json::json!({
            "current_password": "password123",
            "new_password": "a-brand-new-password",
        }))
        .await;
    res.assert_status_ok();

    let fresh = cookie_value(&res, "access_token");
    assert!(!fresh.is_empty());
    assert_ne!(fresh, access, "the cookie must be refreshed, not reused");
    // The new cookie is the one that works.
    server
        .get("/api/auth/profile")
        .add_header("Cookie", format!("access_token={fresh}"))
        .await
        .assert_status_ok();
    server
        .get("/api/auth/profile")
        .add_header("Cookie", format!("access_token={access}"))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}
