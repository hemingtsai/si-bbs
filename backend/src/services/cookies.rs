//! Session cookies.
//!
//! The browser client keeps its session in cookies rather than in `localStorage`,
//! so a cross-site scripting slip cannot read the tokens and they are never sent by
//! anything that is not a same-origin request. The JSON token pair is still returned
//! by the auth endpoints: the API (and every test) authenticates with
//! `Authorization: Bearer`, and the frontend is the only cookie client.
//!
//! `SameSite=Lax` already stops the cookie riding along on a cross-site POST, which
//! is what makes CSRF fail; the double-submit token in [`middleware::csrf`] is the
//! second lock, for browsers or proxies that mishandle SameSite.

use axum::http::HeaderMap;

pub const ACCESS_COOKIE: &str = "access_token";
pub const REFRESH_COOKIE: &str = "refresh_token";
pub const CSRF_COOKIE: &str = "csrf_token";
/// Header the browser client must echo for unsafe requests when it authenticates
/// with a cookie.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// The access cookie is only ever needed by the API; the refresh cookie only by the
/// refresh and logout endpoints. Narrow paths mean fewer requests carry them.
const ACCESS_PATH: &str = "/api";
const REFRESH_PATH: &str = "/api/auth";

/// Build one `Set-Cookie` value.
fn build(
    name: &str,
    value: &str,
    path: &str,
    max_age_secs: i64,
    http_only: bool,
    secure: bool,
) -> String {
    let mut cookie = format!("{name}={value}; Path={path}; Max-Age={max_age_secs}; SameSite=Lax");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// The cookies to set for a fresh session. `csrf` is deliberately readable by
/// JavaScript: the client has to echo it in a header, and that is the point — an
/// attacker who can only make the browser *send* the cookie cannot read it to forge
/// the header.
pub fn session_cookies(
    access: &str,
    refresh: &str,
    csrf: &str,
    access_ttl_secs: i64,
    refresh_ttl_secs: i64,
    secure: bool,
) -> Vec<String> {
    vec![
        build(
            ACCESS_COOKIE,
            access,
            ACCESS_PATH,
            access_ttl_secs,
            true,
            secure,
        ),
        build(
            REFRESH_COOKIE,
            refresh,
            REFRESH_PATH,
            refresh_ttl_secs,
            true,
            secure,
        ),
        build(CSRF_COOKIE, csrf, "/", refresh_ttl_secs, false, secure),
    ]
}

/// Cookies that delete the session.
pub fn clear_cookies(secure: bool) -> Vec<String> {
    vec![
        build(ACCESS_COOKIE, "", ACCESS_PATH, 0, true, secure),
        build(REFRESH_COOKIE, "", REFRESH_PATH, 0, true, secure),
        build(CSRF_COOKIE, "", "/", 0, false, secure),
    ]
}

/// Read one cookie from a `Cookie` header.
///
/// Hand-rolled on purpose: this is a single-header format with no quoting in the
/// values we set, and it avoids pulling in a cookie crate for twenty lines.
pub fn read(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(&format!("{name}="))
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

/// A fresh CSRF token: 32 random bytes, hex.
pub fn new_csrf_token() -> String {
    crate::services::password_reset::generate_token()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with_cookie(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_str(value).unwrap(),
        );
        headers
    }

    #[test]
    fn session_cookies_are_locked_down() {
        let cookies = session_cookies("a", "r", "c", 900, 604800, true);
        assert_eq!(cookies.len(), 3);
        // The tokens are unreadable from JavaScript and only over TLS.
        assert!(cookies[0].starts_with("access_token=a;"));
        assert!(cookies[0].contains("HttpOnly"));
        assert!(cookies[0].contains("Secure"));
        assert!(cookies[0].contains("Path=/api;"));
        assert!(cookies[0].contains("SameSite=Lax"));
        assert!(cookies[1].contains("Path=/api/auth;"));
        // The CSRF token must be readable: the client echoes it in a header.
        assert!(cookies[2].starts_with("csrf_token=c;"));
        assert!(!cookies[2].contains("HttpOnly"));
        assert!(cookies[2].contains("Path=/;"));
    }

    #[test]
    fn clearing_uses_max_age_zero() {
        // Not every deployment is https, and a Secure deletion cookie would be
        // dropped over plain http — leaving the session behind.
        for cookie in clear_cookies(false) {
            assert!(cookie.contains("Max-Age=0"), "{cookie}");
            assert!(!cookie.contains("Secure"), "{cookie}");
        }
    }

    #[test]
    fn reading_finds_the_named_cookie_among_others() {
        let headers = headers_with_cookie("theme=dark; access_token=abc.def.ghi; csrf_token=xyz");
        assert_eq!(
            read(&headers, ACCESS_COOKIE).as_deref(),
            Some("abc.def.ghi")
        );
        assert_eq!(read(&headers, CSRF_COOKIE).as_deref(), Some("xyz"));
        assert_eq!(read(&headers, "missing"), None);
        // An empty value is treated as absent, not as an empty token.
        assert_eq!(
            read(&headers_with_cookie("access_token="), ACCESS_COOKIE),
            None
        );
        assert_eq!(read(&HeaderMap::new(), ACCESS_COOKIE), None);
        // A prefix of a name must not match.
        assert_eq!(read(&headers, "access").as_deref(), None);
    }

    #[test]
    fn csrf_tokens_are_random() {
        assert_ne!(new_csrf_token(), new_csrf_token());
        assert_eq!(new_csrf_token().len(), 64);
    }
}
