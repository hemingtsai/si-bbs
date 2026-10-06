//! The password-reset link has to be a URL an operator can actually send.
//!
//! With no mailer configured, the log line *is* the delivery channel — so the link in
//! it is the product surface. It used to be built with no request headers at all: the
//! binary listens on loopback, so the logged link was `http://localhost:3000/…`,
//! useless to hand to a user.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use common::{register_and_login, test_ctx};

/// Collects everything the handler logs, so the test can assert on the delivered link.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl Capture {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

#[tokio::test]
async fn the_reset_link_uses_the_forwarded_public_origin() {
    let (server, _pool) = test_ctx().await;
    register_and_login(&server, "alice").await;

    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();

    // The handler runs on this thread (current-thread runtime), so the thread-local
    // subscriber sees its events.
    let guard = tracing::subscriber::set_default(subscriber);
    let res = server
        .post("/api/auth/forgot")
        .add_header("X-Forwarded-Proto", "https")
        .add_header("Host", "sibbs.cn")
        .json(&serde_json::json!({ "email": "alice@example.com" }))
        .await;
    drop(guard);

    res.assert_status(StatusCode::ACCEPTED);
    // The token itself must never be in the response…
    assert!(!res.text().contains("token="));

    // …but the logged link must be one a user can open.
    let logged = capture.text();
    assert!(
        logged.contains("https://sibbs.cn/reset?token="),
        "the reset link was not built from the forwarded headers: {logged}"
    );
    assert!(
        !logged.contains("localhost:3000/reset"),
        "the reset link fell back to the loopback address: {logged}"
    );
}

/// `PUBLIC_BASE_URL` wins over anything a client sends, because that is the only value
/// a hostile request cannot influence.
#[tokio::test]
async fn a_configured_public_base_url_wins_over_the_headers() {
    let (server, pool) = test_ctx().await;
    register_and_login(&server, "bob").await;

    // Rebuild the router with a configured origin, over the same database.
    let mut cfg = common::test_config("https://api.github.com");
    cfg.public_base_url = "https://configured.test".into();
    let configured = common::server_with_config(pool, cfg);

    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    configured
        .post("/api/auth/forgot")
        .add_header("X-Forwarded-Proto", "http")
        .add_header("Host", "evil.test")
        .json(&serde_json::json!({ "email": "bob@example.com" }))
        .await;
    drop(guard);

    let logged = capture.text();
    assert!(
        logged.contains("https://configured.test/reset?token="),
        "a spoofable header overrode the configured origin: {logged}"
    );
}
