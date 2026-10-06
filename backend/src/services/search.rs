//! Keyword search planning.
//!
//! Two strategies, chosen by the query itself:
//!
//! * **FTS5** (`trigram` tokenizer) for queries of three characters or more. The
//!   trigram tokenizer is the one that makes CJK work: a Chinese phrase has no
//!   spaces, so a word-based tokenizer would index it as a single opaque token.
//!   Three characters is also the minimum a trigram index can match.
//! * **LIKE** for one or two characters, where FTS5 cannot help. The pattern is
//!   escaped, because an unescaped `%` (or `_`) turns a search into "match
//!   everything" — that was the actual behaviour before.

/// Shortest query the trigram index can answer.
pub const MIN_FTS_CHARS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Full-text search; the value is a quoted FTS5 phrase.
    Fts(String),
    /// Substring search; the value is a ready-to-bind `%…%` pattern.
    Like(String),
}

/// Turn raw user input into a plan, or `None` when there is nothing to search for.
///
/// Extremely long queries are truncated rather than rejected: nobody types 400
/// characters on purpose, and a bounded input keeps the index work bounded.
pub fn plan(raw: &str) -> Option<Plan> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let collapsed: String = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = collapsed.chars().take(200).collect();

    if capped.chars().count() >= MIN_FTS_CHARS {
        // A quoted phrase: FTS5 operators inside it are literal text, so a query full
        // of `"` or `-` or `*` cannot change the shape of the query, and embedded
        // quotes are doubled per the FTS5 grammar.
        let escaped = capped.replace('"', "\"\"");
        Some(Plan::Fts(format!("\"{escaped}\"")))
    } else {
        Some(Plan::Like(format!("%{}%", escape_like(&capped))))
    }
}

/// Escape the LIKE metacharacters (`\`, `%`, `_`) for use with `ESCAPE '\'`.
pub fn escape_like(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_asks_for_nothing() {
        assert_eq!(plan(""), None);
        assert_eq!(plan("   "), None);
        assert_eq!(plan("\t\n"), None);
    }

    #[test]
    fn short_queries_fall_back_to_escaped_like() {
        assert_eq!(plan("a"), Some(Plan::Like("%a%".into())));
        assert_eq!(plan("中文"), Some(Plan::Like("%中文%".into())));
        // Whitespace is collapsed before measuring.
        assert_eq!(plan(" a "), Some(Plan::Like("%a%".into())));
    }

    #[test]
    fn the_wildcards_a_search_box_accepts_are_neutralised() {
        // This is the bug the audit found: `q=%` matched every row.
        assert_eq!(plan("%"), Some(Plan::Like("%\\%%".into())));
        assert_eq!(plan("_"), Some(Plan::Like("%\\_%".into())));
        assert_eq!(plan("a%"), Some(Plan::Like("%a\\%%".into())));
        assert_eq!(plan("a_"), Some(Plan::Like("%a\\_%".into())));
        // Three characters or more never reach LIKE at all: inside an FTS phrase the
        // metacharacters are literal text.
        assert_eq!(plan("100%"), Some(Plan::Fts("\"100%\"".into())));
        assert_eq!(plan("a_b"), Some(Plan::Fts("\"a_b\"".into())));
        assert_eq!(plan("\\"), Some(Plan::Like("%\\\\%".into())));
        // Escaping must not double up.
        assert_eq!(escape_like("a%b_c\\d"), "a\\%b\\_c\\\\d");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn longer_queries_become_a_quoted_fts_phrase() {
        assert_eq!(plan("rust"), Some(Plan::Fts("\"rust\"".into())));
        assert_eq!(plan("中文论坛"), Some(Plan::Fts("\"中文论坛\"".into())));
        // FTS5 syntax characters inside a phrase are literal.
        assert_eq!(plan("a-b*c"), Some(Plan::Fts("\"a-b*c\"".into())));
        assert_eq!(plan("a OR b"), Some(Plan::Fts("\"a OR b\"".into())));
        assert_eq!(
            plan("  rust   search  "),
            Some(Plan::Fts("\"rust search\"".into()))
        );
    }

    #[test]
    fn embedded_quotes_are_doubled_so_the_phrase_cannot_be_escaped_out_of() {
        assert_eq!(
            plan("\"quoted\""),
            Some(Plan::Fts("\"\"\"quoted\"\"\"".into()))
        );
        assert_eq!(
            plan("三\"字\"查询"),
            Some(Plan::Fts("\"三\"\"字\"\"查询\"".into()))
        );
    }

    #[test]
    fn absurdly_long_queries_are_truncated_not_rejected() {
        let long = "x".repeat(500);
        match plan(&long).unwrap() {
            Plan::Fts(phrase) => {
                // 200 characters plus the quoting.
                assert_eq!(phrase.chars().count(), 202);
            }
            other => panic!("expected an FTS plan, got {other:?}"),
        }
    }
}
