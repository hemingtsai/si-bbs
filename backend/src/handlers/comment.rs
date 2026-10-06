use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::error::AppError;
use crate::handlers::project::fetch_public_project;
use crate::middleware::auth::require_auth;
use crate::models::comment::{CommentInput, CommentOut};
use crate::models::page::Page;
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::reports;

const MIN_CONTENT_LEN: usize = 1;
const MAX_CONTENT_LEN: usize = 5000;

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

const COMMENT_SELECT: &str = "SELECT c.id, c.project_id, c.user_id, u.username, c.content, \
     c.created_at FROM comments c JOIN users u ON u.id = c.user_id";

/// Post a comment on an approved project. Login required.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<i64>,
    Json(input): Json<CommentInput>,
) -> Result<(StatusCode, Json<CommentOut>), AppError> {
    let claims = require_auth(&state, &headers).await?;

    let content = input.content.trim();
    let len = content.chars().count();
    if len < MIN_CONTENT_LEN {
        return Err(AppError::BadRequest("comment cannot be empty".into()));
    }
    if len > MAX_CONTENT_LEN {
        return Err(AppError::BadRequest(format!(
            "comment cannot exceed {MAX_CONTENT_LEN} characters"
        )));
    }

    // Pending or rejected projects are not visible, so they cannot be commented on.
    fetch_public_project(&state, project_id).await?;

    let res =
        sqlx::query("INSERT INTO comments (project_id, user_id, content) VALUES (?1, ?2, ?3)")
            .bind(project_id)
            .bind(claims.sub)
            .bind(content)
            .execute(&state.pool)
            .await?;

    let id = res.last_insert_rowid();
    let comment = fetch_comment(&state, id).await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

/// Public, paginated comment list for a project, oldest first.
pub async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Page<CommentOut>>, AppError> {
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);

    fetch_public_project(&state, project_id).await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM comments \
         WHERE project_id = ?1 AND deleted_at IS NULL AND status = 'published'",
    )
    .bind(project_id)
    .fetch_one(&state.pool)
    .await?;

    let sql = format!(
        "{COMMENT_SELECT} WHERE c.project_id = ?1 AND c.deleted_at IS NULL \
         AND c.status = 'published' ORDER BY c.created_at ASC, c.id ASC LIMIT ?2 OFFSET ?3"
    );
    let items: Vec<CommentOut> = sqlx::query_as(&sql)
        .bind(project_id)
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

/// Soft delete a comment. Comment author, moderator or admin only.
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, comment_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    let claims = require_auth(&state, &headers).await?;

    let author_id: i64 = sqlx::query_scalar(
        "SELECT user_id FROM comments WHERE id = ?1 AND project_id = ?2 AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .bind(project_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let staff =
        Role::parse(&claims.role).is_some_and(|r| matches!(r, Role::Admin | Role::Moderator));
    if author_id != claims.sub && !staff {
        return Err(AppError::Forbidden);
    }

    sqlx::query(
        "UPDATE comments SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ?2 \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;

    reports::resolve_for_target(
        &state.pool,
        "comment",
        comment_id,
        claims.sub,
        "content removed",
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_comment(state: &AppState, id: i64) -> Result<CommentOut, AppError> {
    let sql = format!("{COMMENT_SELECT} WHERE c.id = ?1");
    sqlx::query_as::<_, CommentOut>(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)
}
