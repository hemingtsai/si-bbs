//! The deployment's own public origin.
//!
//! Anything that has to hand a *usable* absolute URL to a user — RSS item links, the
//! password-reset link — has to know the scheme and host the request arrived on.
//! Behind a TLS-terminating proxy that information is only in the forwarded headers,
//! and the binary itself listens on loopback, so its own idea of its address is
//! useless.
//!
//! This lives in one place because it was previously spelled out per handler, and the
//! password-reset endpoint passed no headers at all: the link it logged pointed at
//! `http://localhost:3000`, which is not something an operator can send to a user.

use axum::http::{HeaderMap, header};

use crate::services::feed;

/// Request scheme from `X-Forwarded-Proto`, falling back to the `Forwarded` header's
/// `proto=`, then to plain http.
fn scheme(headers: &HeaderMap) -> Option<&str> {
    let header_value = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(proto) = header_value("x-forwarded-proto") {
        // Proxies may append: "https, http".
        return proto.split(',').next().map(str::trim);
    }
    header_value("forwarded").and_then(|value| {
        value
            .split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix("proto="))
            .map(|proto| proto.trim_matches('"'))
    })
}

/// Host from `X-Forwarded-Host` (which wins: the proxy knows the public name) or the
/// `Host` header.
fn host(headers: &HeaderMap) -> Option<&str> {
    let header_value = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    header_value("x-forwarded-host")
        .or_else(|| header_value(header::HOST.as_str()))
        .map(|value| value.split(',').next().unwrap_or(value).trim())
}

/// The origin to build absolute links from.
///
/// `configured` (`PUBLIC_BASE_URL`) wins when set, because it is the only value that
/// cannot be spoofed by a client-supplied header.
pub fn base(configured: &str, headers: &HeaderMap) -> String {
    feed::base_url(configured, scheme(headers), host(headers))
}

/// An absolute URL for `path` (which must start with `/`).
pub fn absolute(configured: &str, headers: &HeaderMap, path: &str) -> String {
    format!("{}{path}", base(configured, headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn the_configured_origin_wins_even_when_it_disagrees_with_the_headers() {
        // The only value a client cannot influence.
        let base = base(
            "https://sibbs.cn/",
            &headers(&[("x-forwarded-proto", "http"), ("host", "evil.test")]),
        );
        assert_eq!(base, "https://sibbs.cn");
    }

    #[test]
    fn forwarded_headers_are_read_when_nothing_is_configured() {
        let h = headers(&[("x-forwarded-proto", "https"), ("host", "sibbs.cn")]);
        assert_eq!(base("", &h), "https://sibbs.cn");
        assert_eq!(
            absolute("", &h, "/reset?token=abc"),
            "https://sibbs.cn/reset?token=abc"
        );
        // The proxy may announce a different public host than the Host header.
        let h = headers(&[
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "sibbs.cn"),
            ("host", "127.0.0.1:3000"),
        ]);
        assert_eq!(base("", &h), "https://sibbs.cn");
        // Comma-separated values from a chain of proxies: the first hop is the client's.
        let h = headers(&[("x-forwarded-proto", "https, http"), ("host", "sibbs.cn")]);
        assert_eq!(base("", &h), "https://sibbs.cn");
        let h = headers(&[
            ("forwarded", "for=1.2.3.4;proto=https"),
            ("host", "sibbs.cn"),
        ]);
        assert_eq!(base("", &h), "https://sibbs.cn");
    }

    #[test]
    fn a_bare_http_request_falls_back_to_the_host_header() {
        let h = headers(&[("host", "localhost:3000")]);
        assert_eq!(base("", &h), "http://localhost:3000");
        // Nothing at all: the binary's own address, which is only useful locally.
        assert_eq!(base("", &HeaderMap::new()), "http://localhost:3000");
    }

    #[test]
    fn https_is_only_assumed_from_a_forwarded_scheme() {
        // Without the header the request is plain http, even for a public host: better
        // a link that redirects than one the browser refuses as mixed content.
        let h = headers(&[("host", "sibbs.cn")]);
        assert_eq!(base("", &h), "http://sibbs.cn");
    }
}
