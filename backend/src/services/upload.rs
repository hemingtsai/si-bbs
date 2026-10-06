//! Attachment storage rules: what may be uploaded, and how the stored file is named.
//!
//! The client's `Content-Type` and filename are both attacker-controlled, so
//! neither decides anything on its own: the type must be on the allowlist **and**
//! match the file's own leading bytes, and the stored name is generated here.

/// Types we are willing to serve back. Deliberately excludes SVG and HTML: both can
/// carry script, and they would be served from our own origin.
pub const ALLOWED: [&str; 7] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "application/pdf",
    "text/plain",
    "text/markdown",
];

/// Identify a body from its magic bytes.
///
/// Returns `None` for a body that matches nothing we allow — including the text
/// types, whose declared type is accepted only because text cannot execute in a
/// browser when served with `nosniff` and a non-HTML type.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    None
}

/// The declared type is trustworthy only when it is on the allowlist and, for the
/// binary formats, corroborated by [`sniff`].
pub fn declared_matches_body(declared: &str, bytes: &[u8]) -> bool {
    let declared = declared
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !ALLOWED.contains(&declared.as_str()) {
        return false;
    }
    match sniff(bytes) {
        Some(sniffed) => sniffed == declared,
        // Text types have no signature; they are allowed as declared as long as the
        // body is valid UTF-8 (so it cannot smuggle arbitrary binary).
        None => {
            matches!(declared.as_str(), "text/plain" | "text/markdown")
                && std::str::from_utf8(bytes).is_ok()
        }
    }
}

/// The extension used for a stored file, derived from the **sniffed** type rather
/// than from the uploaded filename.
pub fn extension_for(content_type: &str) -> &'static str {
    match content_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        "text/markdown" => "md",
        _ => "txt",
    }
}

/// A display name that cannot be confused with a path.
///
/// The upload is served by id, never by this name, so it only has to be safe to put
/// in a header and in the UI: no separators, no control characters, bounded length.
pub fn safe_filename(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\'')
        .take(120)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "upload".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Two-level directory split so one directory never holds every file.
pub fn storage_dir(seed: &str) -> String {
    let head: String = seed.chars().take(4).collect();
    let (a, b) = head.split_at(head.len() / 2);
    format!("{a}/{b}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0, 0];
    const GIF: &[u8] = b"GIF89a.....";
    const WEBP: &[u8] = b"RIFF\x00\x00\x00\x00WEBPVP8 ";
    const PDF: &[u8] = b"%PDF-1.7\n";
    const HTML: &[u8] = b"<html><script>alert(1)</script>";

    #[test]
    fn signatures_are_recognised() {
        assert_eq!(sniff(PNG), Some("image/png"));
        assert_eq!(sniff(JPEG), Some("image/jpeg"));
        assert_eq!(sniff(GIF), Some("image/gif"));
        assert_eq!(sniff(WEBP), Some("image/webp"));
        assert_eq!(sniff(PDF), Some("application/pdf"));
        assert_eq!(sniff(HTML), None);
        assert_eq!(sniff(b"just text"), None);
    }

    #[test]
    fn a_lying_content_type_is_rejected() {
        // The classic trick: call it an image, ship markup.
        assert!(!declared_matches_body("image/png", HTML));
        assert!(declared_matches_body("image/png", PNG));
        assert!(declared_matches_body("image/jpeg; charset=binary", JPEG));
        // Declared image but the bytes are a PDF.
        assert!(!declared_matches_body("image/png", PDF));
        assert!(declared_matches_body("application/pdf", PDF));
    }

    #[test]
    fn types_outside_the_allowlist_never_match() {
        for bad in [
            "text/html",
            "image/svg+xml",
            "application/javascript",
            "application/octet-stream",
            "",
        ] {
            assert!(!declared_matches_body(bad, PNG), "{bad} was accepted");
        }
    }

    #[test]
    fn text_uploads_must_be_utf8() {
        assert!(declared_matches_body("text/plain", "héllo".as_bytes()));
        assert!(!declared_matches_body("text/plain", &[0xff, 0xfe, 0x00]));
    }

    #[test]
    fn stored_extensions_follow_the_sniffed_type() {
        assert_eq!(extension_for("image/png"), "png");
        assert_eq!(extension_for("text/markdown"), "md");
        assert_eq!(extension_for("text/plain"), "txt");
        assert_eq!(extension_for("application/pdf"), "pdf");
    }

    #[test]
    fn filenames_cannot_smuggle_a_path_or_a_header() {
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename("C:\\windows\\evil.exe"), "evil.exe");
        assert_eq!(safe_filename("bad\"name\r\nX: y"), "badnameX: y");
        assert_eq!(safe_filename("   "), "upload");
        assert_eq!(safe_filename(""), "upload");
        assert!(safe_filename(&"a".repeat(500)).chars().count() <= 120);
    }

    #[test]
    fn storage_directories_are_short_and_stable() {
        assert_eq!(storage_dir("abcdef1234"), "ab/cd");
        assert_eq!(storage_dir("abcdef1234"), storage_dir("abcdef9999"));
    }
}
