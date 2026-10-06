use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::error::AppError;
use crate::middleware::auth::{require_auth, require_role};
use crate::models::forum::{
    Board, BoardInfo, CommentInput, FeaturedInput, ForumComment, ForumPostOut, ForumRule,
    ListQuery, PostInput, RuleInput,
};
use crate::models::page::Page;
use crate::models::user::Role;
use crate::routes::AppState;

const MAX_TITLE_LEN: usize = 200;
const MAX_CONTENT_LEN: usize = 50_000;

/// Like and reply counts are **derived**, never stored: `forum_likes` and
/// `forum_comments` are the only source of truth. Keeping a counter column in
/// sync by hand is what let two concurrent toggles drift apart from the rows.
const POST_SELECT: &str = "SELECT p.id, p.board, p.title, p.content, p.author_id, \
     u.username AS author_username, p.is_featured, \
     (SELECT COUNT(*) FROM forum_likes l \
      WHERE l.target_kind = 'post' AND l.target_id = p.id) AS likes_count, \
     (SELECT COUNT(*) FROM forum_comments rc \
      WHERE rc.post_id = p.id AND rc.deleted_at IS NULL) AS comments_count, \
     p.created_at, p.updated_at \
     FROM forum_posts p LEFT JOIN users u ON u.id = p.author_id";

/// Comment projection with its own derived like count.
const COMMENT_SELECT: &str = "SELECT c.id, c.post_id, c.author_id, \
     u.username AS author_username, c.content, \
     (SELECT COUNT(*) FROM forum_likes l \
      WHERE l.target_kind = 'comment' AND l.target_id = c.id) AS likes_count, \
     c.created_at \
     FROM forum_comments c LEFT JOIN users u ON u.id = c.author_id";

fn is_staff(role: &str) -> bool {
    matches!(Role::parse(role), Some(Role::Admin) | Some(Role::Moderator))
}

fn validate_post(input: &PostInput) -> Result<(Board, String, String), AppError> {
    let board = Board::parse(input.board.trim())
        .ok_or_else(|| AppError::BadRequest("board must be models, tools or life".into()))?;
    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if title.chars().count() > MAX_TITLE_LEN {
        return Err(AppError::BadRequest("title is too long".into()));
    }
    let content = input.content.trim();
    if content.is_empty() {
        return Err(AppError::BadRequest("content is required".into()));
    }
    if content.chars().count() > MAX_CONTENT_LEN {
        return Err(AppError::BadRequest("content is too long".into()));
    }
    Ok((board, title.to_string(), content.to_string()))
}

/// Three fixed boards with their live post counts.
pub async fn boards(State(state): State<AppState>) -> Result<Json<Vec<BoardInfo>>, AppError> {
    let mut out = Vec::with_capacity(3);
    for board in Board::ALL {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM forum_posts WHERE board = ?1 AND deleted_at IS NULL",
        )
        .bind(board.as_str())
        .fetch_one(&state.pool)
        .await?;
        out.push(BoardInfo {
            slug: board.as_str().to_string(),
            post_count: count,
        });
    }
    Ok(Json(out))
}

/// Active posts, optionally filtered by board and keyword. Featured first.
pub async fn list_posts(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Page<ForumPostOut>>, AppError> {
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let board =
        match q.board.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(raw) => Some(Board::parse(raw).ok_or_else(|| {
                AppError::BadRequest("board must be models, tools or life".into())
            })?),
        };
    let keyword =
        q.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"));

    let filters = "p.deleted_at IS NULL \
                   AND (?1 IS NULL OR p.board = ?1) \
                   AND (?2 IS NULL OR p.title LIKE ?2 OR p.content LIKE ?2)";

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM forum_posts p WHERE {filters}"
    ))
    .bind(board.map(Board::as_str))
    .bind(keyword.as_deref())
    .fetch_one(&state.pool)
    .await?;

    let sql = format!(
        "{POST_SELECT} WHERE {filters} \
         ORDER BY p.is_featured DESC, p.created_at DESC, p.id DESC LIMIT ?3 OFFSET ?4"
    );
    let items: Vec<ForumPostOut> = sqlx::query_as(&sql)
        .bind(board.map(Board::as_str))
        .bind(keyword.as_deref())
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(Page {
        items,
        total,
        page,
        per_page,
    }))
}

/// Create a post. No moderation: it is visible as soon as it is written.
pub async fn create_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PostInput>,
) -> Result<(StatusCode, Json<ForumPostOut>), AppError> {
    let claims = require_auth(&state, &headers).await?;
    let (board, title, content) = validate_post(&input)?;

    let res = sqlx::query(
        "INSERT INTO forum_posts (board, title, content, author_id) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(board.as_str())
    .bind(&title)
    .bind(&content)
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;

    let id = res.last_insert_rowid();
    Ok((StatusCode::CREATED, Json(fetch_post(&state, id).await?)))
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<ForumPostOut>, AppError> {
    Ok(Json(fetch_post(&state, id).await?))
}

/// Author or staff edits. Only title/board content can change.
pub async fn update_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<PostInput>,
) -> Result<Json<ForumPostOut>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let (board, title, content) = validate_post(&input)?;

    let owner_id: i64 = sqlx::query_scalar(
        "SELECT author_id FROM forum_posts WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if owner_id != claims.sub && !is_staff(&claims.role) {
        return Err(AppError::Forbidden);
    }

    sqlx::query(
        "UPDATE forum_posts SET board = ?2, title = ?3, content = ?4, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(board.as_str())
    .bind(&title)
    .bind(&content)
    .execute(&state.pool)
    .await?;

    Ok(Json(fetch_post(&state, id).await?))
}

/// Author or staff soft-deletes.
pub async fn delete_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let owner_id: i64 = sqlx::query_scalar(
        "SELECT author_id FROM forum_posts WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if owner_id != claims.sub && !is_staff(&claims.role) {
        return Err(AppError::Forbidden);
    }

    let res = sqlx::query(
        "UPDATE forum_posts SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ?2 \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Staff toggles the "featured" flag. Featured posts sort first in lists.
pub async fn set_featured(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<FeaturedInput>,
) -> Result<Json<ForumPostOut>, AppError> {
    require_role(&state, &headers, &[Role::Admin, Role::Moderator]).await?;

    let res = sqlx::query(
        "UPDATE forum_posts SET is_featured = ?2, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(if input.featured { 1 } else { 0 })
    .execute(&state.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(fetch_post(&state, id).await?))
}

/// Comments for one post, oldest first.
pub async fn list_comments(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Page<ForumComment>>, AppError> {
    fetch_post(&state, id).await?;
    let per_page = q.per_page.unwrap_or(50).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM forum_comments WHERE post_id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    let rows: Vec<ForumComment> = sqlx::query_as(&format!(
        "{COMMENT_SELECT} \
         WHERE c.post_id = ?1 AND c.deleted_at IS NULL ORDER BY c.created_at ASC, c.id ASC \
         LIMIT ?2 OFFSET ?3"
    ))
    .bind(id)
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(Page {
        items: rows,
        total,
        page,
        per_page,
    }))
}

/// Create a reply. No moderation.
pub async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<CommentInput>,
) -> Result<(StatusCode, Json<ForumComment>), AppError> {
    let claims = require_auth(&state, &headers).await?;
    let content = input.content.trim();
    if content.is_empty() {
        return Err(AppError::BadRequest("content is required".into()));
    }
    if content.chars().count() > MAX_CONTENT_LEN {
        return Err(AppError::BadRequest("content is too long".into()));
    }
    fetch_post(&state, id).await?;

    let res =
        sqlx::query("INSERT INTO forum_comments (post_id, author_id, content) VALUES (?1, ?2, ?3)")
            .bind(id)
            .bind(claims.sub)
            .bind(content)
            .execute(&state.pool)
            .await?;
    let comment_id = res.last_insert_rowid();

    let comment: ForumComment = sqlx::query_as(&format!("{COMMENT_SELECT} WHERE c.id = ?1"))
        .bind(comment_id)
        .fetch_one(&state.pool)
        .await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

/// Author or staff soft-deletes a reply.
pub async fn delete_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT author_id, post_id FROM forum_comments WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    // `post_id` is no longer needed: the post's reply count is derived from the
    // live rows, so soft-deleting here is enough to change it.
    let (author_id, _post_id) = row.ok_or(AppError::NotFound)?;
    if author_id != claims.sub && !is_staff(&claims.role) {
        return Err(AppError::Forbidden);
    }

    sqlx::query(
        "UPDATE forum_comments SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ?2 \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Toggle a like on a post.
pub async fn like_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    toggle_like(state, headers, "post", id).await
}

async fn toggle_like(
    state: AppState,
    headers: HeaderMap,
    kind: &str,
    id: i64,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let (table, id_col) = match kind {
        "post" => ("forum_posts", "id"),
        "comment" => ("forum_comments", "id"),
        _ => return Err(AppError::NotFound),
    };

    // Target must exist and be live.
    let exists: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT id FROM {table} WHERE {id_col} = ?1 AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let deleted: Option<i64> = sqlx::query_scalar(
        "DELETE FROM forum_likes WHERE user_id = ?1 AND target_kind = ?2 AND target_id = ?3 RETURNING user_id",
    )
    .bind(claims.sub)
    .bind(kind)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    // The toggle is two statements (drop the row if it is there, otherwise add
    // it). Whichever way the race falls, the answer reported below is counted
    // straight off `forum_likes`, so the client never sees a number that the
    // table does not agree with.
    let liked = if deleted.is_some() {
        false
    } else {
        sqlx::query(
            "INSERT OR IGNORE INTO forum_likes (user_id, target_kind, target_id) VALUES (?1, ?2, ?3)",
        )
        .bind(claims.sub)
        .bind(kind)
        .bind(id)
        .execute(&state.pool)
        .await?;
        true
    };

    let likes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM forum_likes WHERE target_kind = ?1 AND target_id = ?2",
    )
    .bind(kind)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(
        serde_json::json!({ "liked": liked, "likes_count": likes }),
    ))
}

/// Toggle a like on a comment.
pub async fn like_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    toggle_like(state, headers, "comment", id).await
}

/// Rules: the global rules plus (optionally) one board's rules.
pub async fn rules(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ForumRule>>, AppError> {
    let board = match q.board.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(raw) => {
            if raw == "global" {
                None
            } else {
                Some(Board::parse(raw).ok_or_else(|| {
                    AppError::BadRequest("board must be global, models, tools or life".into())
                })?)
            }
        }
    };

    let rows: Vec<ForumRule> = match board {
        None => sqlx::query_as(
            "SELECT board, title, content, updated_by, updated_at FROM forum_rules ORDER BY board",
        )
        .fetch_all(&state.pool)
        .await?,
        Some(b) => {
            sqlx::query_as(
                "SELECT board, title, content, updated_by, updated_at FROM forum_rules \
                 WHERE board IN ('global', ?1) ORDER BY board",
            )
            .bind(b.as_str())
            .fetch_all(&state.pool)
            .await?
        }
    };
    Ok(Json(rows))
}

/// Staff replaces a board's rules. `board` path param accepts 'global' too.
pub async fn upsert_rule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(board): Path<String>,
    Json(input): Json<RuleInput>,
) -> Result<Json<ForumRule>, AppError> {
    let claims = require_role(&state, &headers, &[Role::Admin, Role::Moderator]).await?;
    if board != "global" && Board::parse(&board).is_none() {
        return Err(AppError::BadRequest(
            "board must be global, models, tools or life".into(),
        ));
    }
    let title = input.title.trim();
    let content = input.content.trim();
    if title.is_empty() || content.is_empty() {
        return Err(AppError::BadRequest(
            "title and content are required".into(),
        ));
    }

    sqlx::query(
        "INSERT INTO forum_rules (board, title, content, updated_by, updated_at) \
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP) \
         ON CONFLICT(board) DO UPDATE SET title = excluded.title, content = excluded.content, \
         updated_by = excluded.updated_by, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&board)
    .bind(title)
    .bind(content)
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;

    let rule: ForumRule = sqlx::query_as(
        "SELECT board, title, content, updated_by, updated_at FROM forum_rules WHERE board = ?1",
    )
    .bind(&board)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(rule))
}

async fn fetch_post(state: &AppState, id: i64) -> Result<ForumPostOut, AppError> {
    let sql = format!("{POST_SELECT} WHERE p.id = ?1 AND p.deleted_at IS NULL");
    sqlx::query_as::<_, ForumPostOut>(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)
}

impl Board {
    pub const ALL: [Board; 3] = [Board::Models, Board::Tools, Board::Life];
}
