//! RSS 2.0 rendering.
//!
//! Pure functions only: the handlers fetch rows and turn them into [`FeedItem`]s,
//! everything about XML lives here so escaping can be unit-tested directly. Every
//! string in a feed comes from user-supplied content, so escaping is not optional.

use chrono::NaiveDateTime;

/// One `<item>`.
#[derive(Debug, Clone)]
pub struct FeedItem {
    pub title: String,
    /// Absolute URL, e.g. `https://sibbs.cn/forum/12`.
    pub link: String,
    /// Stable identifier; the link is a good choice because the row id is in it.
    pub guid: String,
    pub pub_date: NaiveDateTime,
    pub description: String,
}

/// Channel metadata for one feed.
pub struct Channel<'a> {
    pub title: &'a str,
    pub link: &'a str,
    pub description: &'a str,
    /// Where this feed itself lives, for `<atom:link rel="self">`.
    pub self_link: &'a str,
    pub items: Vec<FeedItem>,
}

/// XML-escape text for use in element content or attributes.
///
/// Control characters other than tab/newline/carriage-return are dropped rather
/// than escaped: XML 1.0 cannot represent them at all, not even as entities, so a
/// single stray `\u{1}` in a post would make the whole feed unparseable.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(ch),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// Cut a body down to something reasonable for a feed reader.
///
/// Counted in characters (not bytes) so a Chinese post is not cut mid-character,
/// collapsing runs of whitespace so a Markdown table does not turn into a wall of
/// blank lines.
pub fn excerpt(body: &str, max_chars: usize) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let cut: String = collapsed.chars().take(max_chars).collect();
    format!("{cut}…")
}

/// RFC 822 timestamp, which is what RSS 2.0 requires. Our timestamps are UTC.
pub fn rfc822(ts: NaiveDateTime) -> String {
    ts.format("%a, %d %b %Y %H:%M:%S +0000").to_string()
}

/// Render a complete RSS 2.0 document.
pub fn render(channel: &Channel<'_>) -> String {
    let mut out = String::with_capacity(1024 + channel.items.len() * 512);
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n<channel>\n");
    out.push_str(&format!(
        "  <title>{}</title>\n  <link>{}</link>\n  <description>{}</description>\n",
        escape(channel.title),
        escape(channel.link),
        escape(channel.description),
    ));
    out.push_str(&format!(
        "  <atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\"/>\n",
        escape(channel.self_link)
    ));
    out.push_str(&format!(
        "  <lastBuildDate>{}</lastBuildDate>\n",
        rfc822(
            channel
                .items
                .first()
                .map_or_else(|| chrono::Utc::now().naive_utc(), |item| item.pub_date)
        )
    ));

    for item in &channel.items {
        out.push_str("  <item>\n");
        out.push_str(&format!("    <title>{}</title>\n", escape(&item.title)));
        out.push_str(&format!("    <link>{}</link>\n", escape(&item.link)));
        out.push_str(&format!(
            "    <guid isPermaLink=\"true\">{}</guid>\n",
            escape(&item.guid)
        ));
        out.push_str(&format!(
            "    <pubDate>{}</pubDate>\n",
            rfc822(item.pub_date)
        ));
        out.push_str(&format!(
            "    <description>{}</description>\n",
            escape(&item.description)
        ));
        out.push_str("  </item>\n");
    }

    out.push_str("</channel>\n</rss>\n");
    out
}

/// Build the absolute base URL (`https://host`) for links inside a feed.
///
/// `PUBLIC_BASE_URL` wins when set, because the reverse proxy's headers are only
/// as trustworthy as the proxy. Otherwise the proxied scheme/host are used, which
/// is what makes links correct on the documented Caddy setup.
pub fn base_url(configured: &str, forwarded_proto: Option<&str>, host: Option<&str>) -> String {
    let configured = configured.trim().trim_end_matches('/');
    if !configured.is_empty() {
        return configured.to_string();
    }
    let host = host.unwrap_or("localhost:3000");
    let scheme = match forwarded_proto {
        Some("https") => "https",
        _ => "http",
    };
    format!("{scheme}://{host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_covers_the_xml_specials() {
        assert_eq!(
            escape(r#"<script>alert("x" & 'y')</script>"#),
            "&lt;script&gt;alert(&quot;x&quot; &amp; &apos;y&apos;)&lt;/script&gt;"
        );
    }

    #[test]
    fn control_characters_are_dropped_not_escaped() {
        // A form feed or a NUL cannot be represented in XML 1.0 at all; leaving
        // one in would make the entire feed unparseable.
        assert_eq!(escape("a\u{0}b\u{c}c\u{1}d"), "abcd");
        assert_eq!(escape("line\nbreak\ttab"), "line\nbreak\ttab");
    }

    #[test]
    fn excerpt_collapses_whitespace_and_cuts_on_char_boundaries() {
        assert_eq!(excerpt("a\n\n  b   c", 40), "a b c");
        assert_eq!(excerpt("abcdefghij", 4), "abcd…");
        // Six Chinese characters are six chars, not eighteen bytes.
        assert_eq!(excerpt("中文标题内容", 3), "中文标…");
    }

    #[test]
    fn timestamps_use_rfc822() {
        let ts = NaiveDateTime::parse_from_str("2026-10-06 13:45:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(rfc822(ts), "Tue, 06 Oct 2026 13:45:00 +0000");
    }

    #[test]
    fn a_rendered_feed_is_well_formed_enough_to_parse() {
        let items = vec![FeedItem {
            title: "标题 & <tag>".into(),
            link: "https://sibbs.cn/forum/1".into(),
            guid: "https://sibbs.cn/forum/1".into(),
            pub_date: NaiveDateTime::parse_from_str("2026-10-06 13:45:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            description: "a < b & c".into(),
        }];
        let xml = render(&Channel {
            title: "SI BBS",
            link: "https://sibbs.cn",
            description: "forum",
            self_link: "https://sibbs.cn/feed.xml",
            items,
        });
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
        assert!(xml.contains("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">"));
        assert!(xml.contains("<title>标题 &amp; &lt;tag&gt;</title>"));
        assert!(xml.contains("<description>a &lt; b &amp; c</description>"));
        assert!(xml.contains("<pubDate>Tue, 06 Oct 2026 13:45:00 +0000</pubDate>"));
        assert!(xml.contains("<atom:link href=\"https://sibbs.cn/feed.xml\" rel=\"self\""));
        assert!(xml.ends_with("</rss>\n"));
    }

    #[test]
    fn base_url_prefers_configuration_over_headers() {
        assert_eq!(
            base_url("https://sibbs.cn/", Some("http"), Some("evil.test")),
            "https://sibbs.cn"
        );
        assert_eq!(
            base_url("", Some("https"), Some("sibbs.cn")),
            "https://sibbs.cn"
        );
        // No proxy headers (direct hit) still produces something usable.
        assert_eq!(
            base_url("", None, Some("localhost:3000")),
            "http://localhost:3000"
        );
        assert_eq!(base_url("", None, None), "http://localhost:3000");
    }
}
