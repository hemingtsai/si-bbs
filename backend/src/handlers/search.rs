//! Global search across wiki pages, forum posts and projects.

use axum::Json;
use axum::extract::{Query, State};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::models::page::Page;
use crate::routes::AppState;
use crate::services::search::{self, Plan};

#[derive(Debug, Deserialize, Default)]
pub struct SearchQuery {
    pub q: Option<String>,
    /// `all` (default), `wiki`, `forum` or `project`.
    pub kind: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

/// One search hit, shaped the same whatever it came from.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Hit {
    pub kind: String,
    pub id: i64,
    pub title: String,
    /// Wiki pages are addressed by slug, so a result can link straight to one.
    /// `NULL` for the other kinds, which are addressed by id.
    pub slug: Option<String>,
    /// Excerpt around the match, produced by FTS5 itself (or the opening of the body
    /// for the short-query fallback).
    pub excerpt: String,
    pub updated_at: NaiveDateTime,
}

/// The three branches of the union, in a fixed order so binding stays predictable.
const KINDS: [&str; 3] = ["wiki", "forum", "project"];

fn parse_kind(raw: Option<&str>) -> Result<Option<&'static str>, AppError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("all") => Ok(None),
        Some("wiki") => Ok(Some("wiki")),
        Some("forum") => Ok(Some("forum")),
        Some("project") => Ok(Some("project")),
        Some(_) => Err(AppError::BadRequest(
            "kind must be all, wiki, forum or project".into(),
        )),
    }
}

/// `GET /api/search?q=&kind=&page=&per_page=`
///
/// Public: it only ever returns content the corresponding list endpoints already
/// expose (published wiki pages, live forum posts, approved projects).
pub async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Page<Hit>>, AppError> {
    let kind = parse_kind(q.kind.as_deref())?;
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * per_page;

    let plan = match search::plan(q.q.as_deref().unwrap_or("")) {
        Some(plan) => plan,
        // No query: an empty page rather than an error, so a search box can call this
        // on every keystroke without special-casing emptiness.
        None => {
            return Ok(Json(Page {
                items: Vec::new(),
                total: 0,
                page,
                per_page,
            }));
        }
    };

    let branches: Vec<&str> = KINDS
        .iter()
        .copied()
        .filter(|k| kind.is_none_or(|wanted| wanted == *k))
        .collect();

    let items: Vec<Hit> = match &plan {
        Plan::Fts(phrase) => {
            let sql = union_sql(
                &branches,
                // `bm25()` is negative-is-better, so ascending order puts the best
                // match first; ties fall back to recency and then to (kind, id), which
                // is what makes LIMIT/OFFSET paging deterministic — without a unique
                // tiebreaker a tied row can appear on both pages.
                "ORDER BY score ASC, updated_at DESC, kind ASC, id ASC LIMIT ?2 OFFSET ?3",
                true,
            );
            sqlx::query_as(&sql)
                .bind(phrase)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&state.pool)
                .await?
        }
        Plan::Like(pattern) => {
            let sql = union_sql(
                &branches,
                "ORDER BY updated_at DESC, kind ASC, id ASC LIMIT ?2 OFFSET ?3",
                false,
            );
            sqlx::query_as(&sql)
                .bind(pattern)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&state.pool)
                .await?
        }
    };

    let total = count(&state, &branches, &plan).await?;

    Ok(Json(Page {
        items,
        total,
        page,
        per_page,
    }))
}

/// Build the union of the requested branches.
///
/// The SQL is assembled from a fixed table of literals — the only variable parts are
/// which branches (from [`KINDS`]) and whether the match is FTS or LIKE — so no
/// request text reaches the query string.
fn union_sql(branches: &[&str], tail: &str, fts: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    for branch in branches {
        let part = if fts {
            match *branch {
                "wiki" => {
                    "SELECT 'wiki' AS kind, w.id AS id, w.title AS title, w.slug AS slug, \
                     snippet(wiki_fts, 1, '', '', '…', 16) AS excerpt, w.updated_at AS updated_at, \
                     bm25(wiki_fts) AS score \
                     FROM wiki_fts JOIN wiki_pages w ON w.id = wiki_fts.rowid \
                     WHERE wiki_fts MATCH ?1 AND w.deleted_at IS NULL AND w.status = 'published'"
                }
                "forum" => {
                    "SELECT 'forum' AS kind, p.id AS id, p.title AS title, NULL AS slug, \
                     snippet(forum_fts, 1, '', '', '…', 16) AS excerpt, p.created_at AS updated_at, \
                     bm25(forum_fts) AS score \
                     FROM forum_fts JOIN forum_posts p ON p.id = forum_fts.rowid \
                     WHERE forum_fts MATCH ?1 AND p.deleted_at IS NULL"
                }
                _ => {
                    "SELECT 'project' AS kind, pr.id AS id, pr.name AS title, NULL AS slug, \
                     snippet(projects_fts, 1, '', '', '…', 16) AS excerpt, pr.updated_at AS updated_at, \
                     bm25(projects_fts) AS score \
                     FROM projects_fts JOIN projects pr ON pr.id = projects_fts.rowid \
                     WHERE projects_fts MATCH ?1 AND pr.deleted_at IS NULL AND pr.status = 'approved'"
                }
            }
        } else {
            match *branch {
                "wiki" => {
                    "SELECT 'wiki' AS kind, w.id AS id, w.title AS title, w.slug AS slug, \
                     substr(w.content, 1, 160) AS excerpt, w.updated_at AS updated_at, 0 AS score \
                     FROM wiki_pages w \
                     WHERE w.deleted_at IS NULL AND w.status = 'published' \
                     AND (w.title LIKE ?1 ESCAPE '\\' OR w.content LIKE ?1 ESCAPE '\\')"
                }
                "forum" => {
                    "SELECT 'forum' AS kind, p.id AS id, p.title AS title, NULL AS slug, \
                     substr(p.content, 1, 160) AS excerpt, p.created_at AS updated_at, 0 AS score \
                     FROM forum_posts p \
                     WHERE p.deleted_at IS NULL \
                     AND (p.title LIKE ?1 ESCAPE '\\' OR p.content LIKE ?1 ESCAPE '\\')"
                }
                _ => {
                    "SELECT 'project' AS kind, pr.id AS id, pr.name AS title, NULL AS slug, \
                     substr(IFNULL(pr.description, ''), 1, 160) AS excerpt, \
                     pr.updated_at AS updated_at, 0 AS score \
                     FROM projects pr \
                     WHERE pr.deleted_at IS NULL AND pr.status = 'approved' \
                     AND (pr.name LIKE ?1 ESCAPE '\\' OR IFNULL(pr.description, '') LIKE ?1 ESCAPE '\\')"
                }
            }
        };
        parts.push(part.to_string());
    }
    format!("SELECT * FROM ({}) {tail}", parts.join(" UNION ALL "))
}

async fn count(state: &AppState, branches: &[&str], plan: &Plan) -> Result<i64, AppError> {
    let mut total = 0i64;
    for branch in branches {
        let (table, filter, columns) = match *branch {
            "wiki" => (
                "wiki_pages",
                "deleted_at IS NULL AND status = 'published'",
                "title, content",
            ),
            "forum" => ("forum_posts", "deleted_at IS NULL", "title, content"),
            _ => (
                "projects",
                "deleted_at IS NULL AND status = 'approved'",
                "name, IFNULL(description, '')",
            ),
        };
        let sql = match plan {
            // `MATCH` needs the FTS table's own name, not an alias.
            Plan::Fts(_) => format!(
                "SELECT COUNT(*) FROM {table} t JOIN {fts} ON {fts}.rowid = t.id \
                 WHERE {filter} AND {fts} MATCH ?1",
                fts = fts_table(branch)
            ),
            Plan::Like(_) => {
                let (a, b) = split_columns(columns);
                format!(
                    "SELECT COUNT(*) FROM {table} WHERE {filter} \
                     AND ({a} LIKE ?1 ESCAPE '\\' OR {b} LIKE ?1 ESCAPE '\\')"
                )
            }
        };
        let value = match plan {
            Plan::Fts(phrase) => phrase.as_str(),
            Plan::Like(pattern) => pattern.as_str(),
        };
        let n: i64 = sqlx::query_scalar(&sql)
            .bind(value)
            .fetch_one(&state.pool)
            .await?;
        total += n;
    }
    Ok(total)
}

fn fts_table(branch: &str) -> &'static str {
    match branch {
        "wiki" => "wiki_fts",
        "forum" => "forum_fts",
        _ => "projects_fts",
    }
}

/// `"title, content"` → `("title", "content")`, without leaking the split into the
/// query text (both halves are literals from this module).
fn split_columns(columns: &str) -> (String, String) {
    match columns.split_once(", ") {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (columns.to_string(), columns.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_sql_never_interpolates_request_text() {
        let sql = union_sql(
            &["wiki"],
            "ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3",
            true,
        );
        assert!(sql.contains("wiki_fts MATCH ?1"));
        assert!(!sql.contains("'wiki_fts MATCH"));
        assert!(sql.ends_with("ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"));
        // Only the kinds we asked for appear.
        assert!(!sql.contains("forum_posts"));
        assert!(!sql.contains("projects"));
    }

    #[test]
    fn the_like_branches_escape_their_wildcards() {
        for branch in KINDS {
            let sql = union_sql(&[branch], "LIMIT ?2", false);
            assert!(sql.contains("ESCAPE '\\'"), "{branch} does not escape LIKE");
            assert!(sql.contains("?1"), "{branch} is not parameterised");
        }
    }

    #[test]
    fn kind_parsing_rejects_nonsense() {
        assert_eq!(parse_kind(None).unwrap(), None);
        assert_eq!(parse_kind(Some("all")).unwrap(), None);
        assert_eq!(parse_kind(Some(" forum ")).unwrap(), Some("forum"));
        assert!(parse_kind(Some("users")).is_err());
    }
}
