use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

/// Security headers and immutable caching for hashed assets.
///
/// Applied as a middleware so even the dev-mode binary (no Caddy/nginx in
/// front) emits them; the production proxies then see consistent headers.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    let h = res.headers_mut();

    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'",
        ),
    );

    // Hashed /assets/* can be cached forever; everything else revalidates. A
    // handler that set its own policy wins — attachments, for instance, are
    // immutable too, and blindly overwriting would have made them uncacheable.
    if !h.contains_key("cache-control") {
        if path.starts_with("/assets/") {
            h.insert(
                "cache-control",
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
        } else {
            h.insert("cache-control", HeaderValue::from_static("no-cache"));
        }
    }

    // A static response may come from the `.br`/`.gz` sibling depending on the
    // request's `Accept-Encoding` — tower-http picks the variant but does not
    // advertise that the representation varies, so a shared cache could hand a
    // Brotli body to a client that cannot decode it. API responses are built per
    // request and never encoded, so they do not need this.
    if !path.starts_with("/api/") {
        h.insert("vary", HeaderValue::from_static("accept-encoding"));
    }

    res
}
