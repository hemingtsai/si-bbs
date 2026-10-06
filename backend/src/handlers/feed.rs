//! RSS feeds: a site-wide one plus one per content type.
//!
//! Feeds are public and deliberately unauthenticated — a feed reader cannot send
//! an `Authorization` header. They only expose what the corresponding list
//! endpoints already expose publicly, and hard-code the limit so a subscriber
//! cannot ask for the whole table by polling `?per_page=100`.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::forum::Board;
use crate::routes::AppState;
use crate::services::feed::{self, Channel, FeedItem};
use crate::services::public_url;

/// Items per feed. Small enough that polling costs nothing, large enough to be
/// useful in a reader.
const FEED_LIMIT: i64 = 30;
/// Bodies are quoted, not mirrored: a wiki page can be 200KB of Markdown.
const EXCERPT_CHARS: usize = 600;

#[derive(Debug, Deserialize, Default)]
pub struct FeedQuery {
    pub board: Option<String>,
}

/// `GET /feed.xml` — everything, newest first.
pub async fn site(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let base = base_for(&state, &headers);

    let forum: Vec<FeedItem> = sqlx::query_as::<_, Row>(
        "SELECT p.title AS title, p.id AS id, p.content AS body, p.created_at AS created_at \
         FROM forum_posts p WHERE p.deleted_at IS NULL \
         ORDER BY p.created_at DESC, p.id DESC LIMIT ?1",
    )
    .bind(FEED_LIMIT)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| row.into_item(&base, "/forum/"))
    .collect();

    let wiki: Vec<FeedItem> = sqlx::query_as::<_, Row>(
        "SELECT w.title AS title, w.id AS id, w.slug AS slug, w.content AS body, \
         w.updated_at AS created_at \
         FROM wiki_pages w WHERE w.deleted_at IS NULL AND w.status = 'published' \
         ORDER BY w.updated_at DESC, w.id DESC LIMIT ?1",
    )
    .bind(FEED_LIMIT)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| row.into_item(&base, "/wiki/"))
    .collect();

    let mut items = forum;
    items.extend(wiki);
    items.sort_by_key(|item| std::cmp::Reverse(item.pub_date));
    items.truncate(FEED_LIMIT as usize);

    Ok(feed_response(feed::render(&Channel {
        title: "SI BBS",
        link: &base,
        description: "Wiki、项目索引与论坛的最新内容",
        self_link: &format!("{base}/feed.xml"),
        items,
    })))
}

/// `GET /forum/feed.xml[?board=models]` — one board, or all of them.
pub async fn forum(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FeedQuery>,
) -> Result<Response, AppError> {
    let base = base_for(&state, &headers);
    let board =
        match q.board.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(raw) => Some(Board::parse(raw).ok_or_else(|| {
                AppError::BadRequest("board must be models, tools or life".into())
            })?),
        };

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT p.title AS title, p.id AS id, p.content AS body, p.created_at AS created_at \
         FROM forum_posts p \
         WHERE p.deleted_at IS NULL AND (?1 IS NULL OR p.board = ?1) \
         ORDER BY p.created_at DESC, p.id DESC LIMIT ?2",
    )
    .bind(board.map(Board::as_str))
    .bind(FEED_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|row| row.into_item(&base, "/forum/"))
        .collect();
    let (self_link, title) = match board {
        Some(b) => (
            format!("{base}/forum/feed.xml?board={}", b.as_str()),
            format!("SI BBS · 论坛 · {}", b.as_str()),
        ),
        None => (
            format!("{base}/forum/feed.xml"),
            "SI BBS · 论坛".to_string(),
        ),
    };

    Ok(feed_response(feed::render(&Channel {
        title: &title,
        link: &format!("{base}/forum"),
        description: "论坛新帖（发帖免审核）",
        self_link: &self_link,
        items,
    })))
}

/// `GET /wiki/feed.xml` — published pages, ordered by last edit.
pub async fn wiki(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let base = base_for(&state, &headers);

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT w.title AS title, w.id AS id, w.slug AS slug, w.content AS body, \
         w.updated_at AS created_at \
         FROM wiki_pages w WHERE w.deleted_at IS NULL AND w.status = 'published' \
         ORDER BY w.updated_at DESC, w.id DESC LIMIT ?1",
    )
    .bind(FEED_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|row| row.into_item(&base, "/wiki/"))
        .collect();

    Ok(feed_response(feed::render(&Channel {
        title: "SI BBS · Wiki",
        link: &format!("{base}/wiki"),
        description: "Wiki 页面变更",
        self_link: &format!("{base}/wiki/feed.xml"),
        items,
    })))
}

/// `GET /projects/feed.xml` — newly accepted projects.
pub async fn projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let base = base_for(&state, &headers);

    let items: Vec<FeedItem> = sqlx::query_as::<_, Row>(
        "SELECT name AS title, id, IFNULL(description, '') AS body, updated_at AS created_at \
         FROM projects WHERE deleted_at IS NULL AND status = 'approved' \
         ORDER BY updated_at DESC, id DESC LIMIT ?1",
    )
    .bind(FEED_LIMIT)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| row.into_item(&base, "/projects/"))
    .collect();

    Ok(feed_response(feed::render(&Channel {
        title: "SI BBS · 项目",
        link: &format!("{base}/projects"),
        description: "新收录的开源项目",
        self_link: &format!("{base}/projects/feed.xml"),
        items,
    })))
}

/// Row shape shared by the queries above; `slug` is only selected by the wiki one.
#[derive(sqlx::FromRow)]
struct Row {
    title: String,
    id: i64,
    #[sqlx(default)]
    slug: Option<String>,
    body: String,
    created_at: chrono::NaiveDateTime,
}

impl Row {
    /// Build a feed item. `prefix` is the route prefix (`/forum/`), and the row's
    /// slug is used when it has one so wiki links stay readable.
    fn into_item(self, base: &str, prefix: &str) -> FeedItem {
        let path = match self.slug.as_deref().filter(|s| !s.is_empty()) {
            Some(slug) => format!("{prefix}{slug}"),
            None => format!("{prefix}{}", self.id),
        };
        let link = format!("{base}{path}");
        FeedItem {
            title: self.title,
            guid: link.clone(),
            link,
            pub_date: self.created_at,
            description: feed::excerpt(&self.body, EXCERPT_CHARS),
        }
    }
}

/// Absolute links for feeds come from the same place as every other public URL, so a
/// single implementation decides what "this deployment's origin" means.
fn base_for(state: &AppState, headers: &HeaderMap) -> String {
    public_url::base(&state.cfg.public_base_url, headers)
}

/// Feeds are XML, not JSON — and the security middleware has already added the
/// usual headers by the time this returns.
fn feed_response(body: String) -> Response {
    (
        [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
        body,
    )
        .into_response()
}
