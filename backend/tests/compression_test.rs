//! On-the-fly compression of API responses.
//!
//! Static files are pre-compressed by the Vite build, so this only has to cover
//! JSON payloads — the wiki pages and project READMEs that are large enough for
//! compression to pay for itself.

mod common;

use axum::http::StatusCode;
use common::{register_and_login, test_ctx};

#[tokio::test]
async fn large_api_responses_are_compressed_and_advertise_it() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let content = "内容".repeat(4000); // ~24KB of UTF-8
    let created = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "big page",
            "category": "c",
            "content": content,
            "status": "published",
        }))
        .await;
    created.assert_status(StatusCode::CREATED);
    let slug = created.json::<serde_json::Value>()["slug"]
        .as_str()
        .unwrap()
        .to_string();

    let res = server
        .get(&format!("/api/wiki/{slug}"))
        .add_header("accept-encoding", "gzip")
        .await;
    res.assert_status_ok();
    assert_eq!(
        res.headers()
            .get("content-encoding")
            .and_then(|v| v.to_str().ok()),
        Some("gzip")
    );
    // Caches must key on the encoding, or a compressed body could be handed to a
    // client that cannot read it.
    assert!(
        res.headers()
            .get("vary")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("accept-encoding")),
        "missing Vary: accept-encoding"
    );
    // The body really is compressed data, not the JSON with a header stapled on:
    // the encoded bytes are smaller than the raw content alone. (`axum-test` does
    // not decompress, so the payload cannot be parsed back here; the identity test
    // below covers the round trip.)
    assert!(
        res.as_bytes().len() < content.len(),
        "body is {} bytes, raw content alone is {}",
        res.as_bytes().len(),
        content.len()
    );
}

#[tokio::test]
async fn small_api_responses_are_not_compressed() {
    let (server, _pool) = test_ctx().await;

    // Two bytes: a gzip round trip would cost more than it saves.
    let res = server
        .get("/api/health")
        .add_header("accept-encoding", "gzip")
        .await;
    res.assert_status_ok();
    assert!(res.headers().get("content-encoding").is_none());
}

#[tokio::test]
async fn a_client_that_accepts_no_encoding_is_never_compressed() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let content = "内容".repeat(4000);
    let created = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "big page",
            "category": "c",
            "content": content,
            "status": "published",
        }))
        .await;
    let slug = created.json::<serde_json::Value>()["slug"]
        .as_str()
        .unwrap()
        .to_string();

    let res = server
        .get(&format!("/api/wiki/{slug}"))
        .add_header("accept-encoding", "identity")
        .await;
    res.assert_status_ok();
    assert!(res.headers().get("content-encoding").is_none());
    assert_eq!(res.json::<serde_json::Value>()["title"], "big page");
}
