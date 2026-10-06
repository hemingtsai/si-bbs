//! Static-file serving: SPA deep-link fallback and the response headers the
//! hashed bundle depends on.

mod common;

use std::fs;

use axum::http::StatusCode;

/// Build a throwaway `dist/` with an index and one hashed asset.
fn static_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("index.html"), "<!doctype html><title>SI BBS</title>")
        .expect("write index.html");
    fs::create_dir_all(dir.path().join("assets")).expect("mkdir assets");
    fs::write(dir.path().join("assets/app-abc123.js"), "console.log(1)").expect("write asset");
    dir
}

#[tokio::test]
async fn hashed_assets_are_immutable_and_carry_the_security_headers() {
    let dir = static_dir();
    let pool = common::test_pool().await;
    let server = common::server_with_static(pool, Some(dir.path().to_string_lossy().into_owned()));

    let res = server.get("/assets/app-abc123.js").await;
    res.assert_status_ok();
    let headers = res.headers();

    // The layer used to be applied before the static fallback was registered, so
    // `Router::layer` never wrapped it and static responses came back without any
    // of these.
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(
        headers.get("x-content-type-options").and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert!(headers.get("content-security-policy").is_some());
}

#[tokio::test]
async fn client_side_routes_fall_back_to_the_shell_without_long_caching() {
    let dir = static_dir();
    let pool = common::test_pool().await;
    let server = common::server_with_static(pool, Some(dir.path().to_string_lossy().into_owned()));

    // A deep link must not 404 on refresh, and index.html must not be cached for
    // a year or a deploy would never be picked up.
    let res = server.get("/forum/42").await;
    res.assert_status_ok();
    assert!(res.text().contains("SI BBS"));
    assert_eq!(
        res.headers().get("cache-control").and_then(|v| v.to_str().ok()),
        Some("no-cache")
    );
}

#[tokio::test]
async fn unknown_api_paths_still_answer_json_not_the_shell() {
    let dir = static_dir();
    let pool = common::test_pool().await;
    let server = common::server_with_static(pool, Some(dir.path().to_string_lossy().into_owned()));

    let res = server.get("/api/does-not-exist").await;
    res.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(res.json::<serde_json::Value>()["error"], "not found");
}

#[tokio::test]
async fn without_a_static_directory_the_api_still_works() {
    let pool = common::test_pool().await;
    let server = common::server_with_static(pool, None);

    server.get("/api/health").await.assert_status_ok();
    // No fallback is registered, so this is a bare 404 rather than the shell.
    server.get("/forum/42").await.assert_status(StatusCode::NOT_FOUND);
}
