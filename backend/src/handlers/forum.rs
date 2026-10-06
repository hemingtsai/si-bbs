use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

use crate::error::AppError;
use crate::middleware::auth::{require_auth, require_role};
use crate::models::forum::{Board, BoardInfo, CommentInput, FeaturedInput, ForumComment, ForumPostOut, ForumRule, ListQuery, PostInput, RuleInput};
use crate::models::page::Page;
use crate::models::user::Role;
use crate::routes::AppState;

const MAX_TITLE_LEN: usize = 200;
const MAX_CONTENT_LEN: usize = 50_000;

const POST_SELECT: &str = "SELECT p.id, p.board, p.title, p.content, p.author_id, \
     u.username AS author_username, p.is_featured, p.likes_count, p.comments_count, \
     p.created_at, p.updated_at \
     FROM forum_posts p LEFT JOIN users u ON u.id = p.author_id";

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
    let board = match q.board.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(raw) => Some(Board::parse(raw).ok_or_else(|| {
            AppError::BadRequest("board must be models, tools or life".into())
        })?),
    };
    let keyword = q
        .q
        .as_deref()
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
    let claims = require_auth(&state.cfg, &headers)?;
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
    let claims = require_auth(&state.cfg, &headers)?;
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
    let claims = require_auth(&state.cfg, &headers)?;
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
    let claims = require_auth(&state.cfg, &headers)?;
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

    let liked = if deleted.is_some() {
        liked_delta(&state, table, id, -1).await?;
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
        liked_delta(&state, table, id, 1).await?;
        true
    };

    let likes: i64 = sqlx::query_scalar(&format!(
        "SELECT likes_count FROM {table} WHERE {id_col} = ?1"
    ))
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({ "liked": liked, "likes_count": likes })))
}

async fn liked_delta(
    state: &AppState,
    table: &str,
    id: i64,
    delta: i64,
) -> Result<(), AppError> {
    sqlx::query(&format!(
        "UPDATE {table} SET likes_count = MAX(likes_count + ?2, 0) WHERE id = ?1"
    ))
    .bind(id)
    .bind(delta)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn fetch_post(state: &AppState, id: i64) -> Result<ForumPostOut, AppError> {
    let sql = format!(
        "{POST_SELECT} WHERE p.id = ?1 AND p.deleted_at IS NULL"
    );
    sqlx::query_as::<_, ForumPostOut>(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)
}

impl Board {
    pub const ALL: [Board; 3] = [Board::Models, Board::Tools, Board::Life];
}
