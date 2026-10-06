//! Attachments: what may be uploaded, what comes back, and who may delete it.

mod common;

use axum::http::StatusCode;
use common::{register_and_login, register_login_as_role, test_ctx};

/// A minimal but real PNG signature plus filler.
const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 9, 9, 9,
];
const HTML: &[u8] = b"<html><script>alert(document.cookie)</script></html>";

/// Build a multipart body by hand: axum-test can send bytes, and this keeps the
/// test independent of any client-side multipart helper.
fn multipart(filename: &str, content_type: &str, body: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----sibbsboundary";
    let mut out = Vec::new();
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    out.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), out)
}

async fn upload(
    server: &axum_test::TestServer,
    token: &str,
    filename: &str,
    content_type: &str,
    body: &[u8],
) -> axum_test::TestResponse {
    let (ct, bytes) = multipart(filename, content_type, body);
    server
        .post("/api/attachments")
        .add_header("Authorization", format!("Bearer {token}"))
        .add_header("Content-Type", ct)
        .bytes(bytes.into())
        .await
}

#[tokio::test]
async fn uploading_an_image_returns_a_ready_to_paste_snippet() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let res = upload(&server, &alice, "screenshot.png", "image/png", PNG).await;
    res.assert_status(StatusCode::CREATED);
    let body = res.json::<serde_json::Value>();
    let id = body["id"].as_i64().unwrap();
    assert_eq!(body["content_type"], "image/png");
    assert_eq!(body["filename"], "screenshot.png");
    assert_eq!(body["size_bytes"], PNG.len() as i64);
    assert_eq!(body["url"], format!("/api/attachments/{id}"));
    assert_eq!(
        body["markdown"],
        format!("![screenshot.png](/api/attachments/{id})")
    );

    // The bytes come back with the stored type, inline, cacheable.
    let served = server.get(&format!("/api/attachments/{id}")).await;
    served.assert_status_ok();
    let headers = served.headers();
    assert_eq!(
        headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("image/png")
    );
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|v| v.to_str().ok()),
        Some("inline")
    );
    assert!(
        headers
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("immutable"))
    );

    // A marking upload is authenticated.
    server
        .get("/api/attachments/999999")
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_file_that_lies_about_its_type_is_refused() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    // Declared as an image, actually markup: this is the payload that would run in
    // the browser if the declared type were trusted.
    let res = upload(&server, &alice, "innocent.png", "image/png", HTML).await;
    res.assert_status(StatusCode::BAD_REQUEST);

    // A PDF declared as a PNG.
    let res = upload(&server, &alice, "doc.png", "image/png", b"%PDF-1.7\n").await;
    res.assert_status(StatusCode::BAD_REQUEST);

    // Types outside the allowlist never pass, whatever the bytes are.
    for bad in ["text/html", "image/svg+xml", "application/javascript"] {
        upload(&server, &alice, "x", bad, PNG)
            .await
            .assert_status(StatusCode::BAD_REQUEST);
    }

    // Nothing was written.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments")
        .fetch_one(&_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn uploads_need_a_session_a_file_and_a_size_within_the_limit() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    // No token.
    let (ct, bytes) = multipart("x.png", "image/png", PNG);
    server
        .post("/api/attachments")
        .add_header("Content-Type", ct)
        .bytes(bytes.into())
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    // Empty file.
    upload(&server, &alice, "empty.png", "image/png", &[])
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // Too large: the limit is enforced before anything is written.
    let big = vec![0u8; 6 * 1024 * 1024];
    let mut with_png_header = PNG.to_vec();
    with_png_header.extend_from_slice(&big);
    upload(&server, &alice, "big.png", "image/png", &with_png_header)
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // A body with no `file` field at all.
    let boundary = "----emptyfield";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nhello\r\n--{boundary}--\r\n"
    );
    server
        .post("/api/attachments")
        .add_header("Authorization", format!("Bearer {alice}"))
        .add_header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .bytes(body.into_bytes().into())
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

/// A path-traversing filename must not decide where the file lands; the stored name
/// is generated and only the label keeps the original.
#[tokio::test]
async fn the_stored_path_is_generated_and_the_label_is_sanitised() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let res = upload(
        &server,
        &alice,
        "../../../etc/passwd",
        "text/plain",
        b"hello",
    )
    .await;
    res.assert_status(StatusCode::CREATED);
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["filename"], "passwd");
    assert_eq!(body["content_type"], "text/plain");

    let stored: String = sqlx::query_scalar("SELECT storage_path FROM attachments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(stored.ends_with(".txt"), "unexpected stored path {stored}");
    assert!(!stored.contains(".."), "traversal survived: {stored}");
    assert!(!stored.starts_with('/'));
    // Two levels of directories, all server-generated hex.
    assert_eq!(stored.split('/').count(), 3, "path shape: {stored}");

    // Text downloads rather than rendering in our origin.
    let served = server
        .get(&format!("/api/attachments/{}", body["id"]))
        .await;
    assert!(
        served
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("attachment"))
    );
}

#[tokio::test]
async fn deleting_an_attachment_is_for_the_uploader_or_staff_and_lands_in_the_bin() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let admin = register_login_as_role(&server, &pool, "root", "admin").await;

    let id = upload(&server, &alice, "mine.png", "image/png", PNG)
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    // Somebody else cannot delete it.
    server
        .delete(&format!("/api/attachments/{id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .delete(&format!("/api/attachments/{id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Gone from the API…
    server
        .get(&format!("/api/attachments/{id}"))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    // …but in the bin, and restorable like everything else.
    let trash = server
        .get("/api/trash")
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .json::<Vec<serde_json::Value>>();
    assert!(
        trash
            .iter()
            .any(|item| item["kind"] == "attachment" && item["id"] == id)
    );

    server
        .post(&format!("/api/trash/attachment/{id}/restore"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status_ok();
    server
        .get(&format!("/api/attachments/{id}"))
        .await
        .assert_status_ok();
}
