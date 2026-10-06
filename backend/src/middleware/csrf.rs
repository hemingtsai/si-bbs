//! CSRF guard for cookie-authenticated writes.
//!
//! A request that authenticates with a cookie is subject to being sent by a
//! cross-site page; a request that carries `Authorization: Bearer` is not, because a
//! hostile page cannot set that header. So the rule is:
//!
//! * unsafe method + `access_token` cookie + **no** `Authorization` header
//!   → require `x-csrf-token` to equal the `csrf_token` cookie;
//! * anything else → untouched.
//!
//! This runs as a layer rather than inside `require_auth` so it is impossible to
//! forget on a new endpoint: the check is about the shape of the request, not about
//! what the handler does.

use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::routes::AppState;
use crate::services::cookies;

fn is_unsafe(method: &Method) -> bool {
    !matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

pub async fn require_csrf(
    State(_state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if !is_unsafe(request.method()) {
        return next.run(request).await;
    }

    let headers: &HeaderMap = request.headers();
    // Bearer requests are not forgeable cross-site, and an absent cookie means the
    // caller is not cookie-authenticated at all.
    let cookie_authenticated = headers.get(axum::http::header::AUTHORIZATION).is_none()
        && cookies::read(headers, cookies::ACCESS_COOKIE).is_some();
    if !cookie_authenticated {
        return next.run(request).await;
    }

    let presented = headers
        .get(cookies::CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let expected = cookies::read(headers, cookies::CSRF_COOKIE).unwrap_or_default();
    // Both must be present and equal. An empty expected value would make a forged
    // request with no header pass, so it is rejected explicitly.
    if expected.is_empty() || presented.is_empty() || presented != expected {
        tracing::warn!(
            path = %request.uri().path(),
            "refused a cookie-authenticated write without a matching CSRF token"
        );
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": "missing or invalid CSRF token",
                "code": "csrf",
            })),
        )
            .into_response();
    }

    next.run(request).await
}
