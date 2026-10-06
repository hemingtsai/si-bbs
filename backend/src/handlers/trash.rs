use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

use crate::error::AppError;
use crate::middleware::auth::{require_auth, require_role};
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::audit;

/// The soft-deletable entity kinds. `trash_view` unions exactly these, and a test
/// asserts the two stay in step: a kind present in the view but missing here
/// cannot be restored or purged, and one listed here but absent from the view
/// makes both operations answer 404 forever.
pub const KINDS: [&str; 6] = [
    "wiki",
    "project",
    "comment",
    "forum_post",
    "forum_comment",
    "attachment",
];

/// Map a public kind to its table. Whitelisted so the name can be interpolated
/// into SQL safely; SQLite cannot bind identifiers.
fn table_for(kind: &str) -> Result<&'static str, AppError> {
    match kind {
        "wiki" => Ok("wiki_pages"),
        "project" => Ok("projects"),
        "comment" => Ok("comments"),
        "forum_post" => Ok("forum_posts"),
        "forum_comment" => Ok("forum_comments"),
        "attachment" => Ok("attachments"),
        _ => Err(AppError::NotFound),
    }
}

fn is_staff(role: &str) -> bool {
    matches!(Role::parse(role), Some(Role::Admin) | Some(Role::Moderator))
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TrashItem {
    pub kind: String,
    pub id: i64,
    pub name: Option<String>,
    pub deleted_at: Option<NaiveDateTime>,
    pub deleted_by: Option<i64>,
    pub deleted_by_username: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RawTrashItem {
    kind: String,
    id: i64,
    name: Option<String>,
    deleted_at: Option<NaiveDateTime>,
    deleted_by: Option<i64>,
    deleted_by_username: Option<String>,
}

/// Deleted items. Users see what they deleted; moderators and admins see all.
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TrashItem>>, AppError> {
    let claims = require_auth(&state, &headers).await?;

    let (sql, mine): (&str, Option<i64>) = if is_staff(&claims.role) {
        (
            "SELECT t.kind, t.id, t.name, t.deleted_at, t.deleted_by, u.username AS deleted_by_username \
             FROM trash_view t LEFT JOIN users u ON u.id = t.deleted_by \
             ORDER BY t.deleted_at DESC, t.kind ASC, t.id DESC",
            None,
        )
    } else {
        (
            "SELECT t.kind, t.id, t.name, t.deleted_at, t.deleted_by, u.username AS deleted_by_username \
             FROM trash_view t LEFT JOIN users u ON u.id = t.deleted_by WHERE t.deleted_by = ?1 \
             ORDER BY t.deleted_at DESC, t.kind ASC, t.id DESC",
            Some(claims.sub),
        )
    };

    let rows: Vec<RawTrashItem> = sqlx::query_as(sql)
        .bind(mine)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| TrashItem {
                kind: r.kind,
                id: r.id,
                name: r.name,
                deleted_at: r.deleted_at,
                deleted_by: r.deleted_by,
                deleted_by_username: r.deleted_by_username,
            })
            .collect(),
    ))
}

async fn authorize_restore(
    state: &AppState,
    claims: &crate::services::auth::Claims,
    kind: &str,
    id: i64,
) -> Result<(), AppError> {
    let table = table_for(kind)?;
    let sql = format!("SELECT deleted_by FROM {table} WHERE id = ?1 AND deleted_at IS NOT NULL");
    let deleted_by: Option<Option<i64>> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let deleted_by = deleted_by.ok_or(AppError::NotFound)?;
    match deleted_by {
        Some(by) if by == claims.sub => Ok(()),
        _ if is_staff(&claims.role) => Ok(()),
        _ => Err(AppError::Forbidden),
    }
}

/// Bring a soft-deleted item back. The user who deleted it, or staff, may
/// restore it. A restored item can collide with a live row that took its unique
/// slug or GitHub URL in the meantime, which surfaces as 409.
pub async fn restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, i64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::NotFound);
    }
    authorize_restore(&state, &claims, &kind, id).await?;

    let table = table_for(&kind)?;
    let sql = format!(
        "UPDATE {table} SET deleted_at = NULL, deleted_by = NULL \
         WHERE id = ?1 AND deleted_at IS NOT NULL"
    );
    let res = sqlx::query(&sql)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => AppError::Conflict(
                "cannot restore: another live item already uses this slug or URL".into(),
            ),
            other => AppError::from(other),
        })?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    // Restoring your own deletion is routine; a moderator digging someone else's
    // content out of the bin is worth recording.
    if is_staff(&claims.role) {
        audit::record_best_effort(
            &state.pool,
            claims.sub,
            audit::TRASH_RESTORE,
            &kind,
            Some(id),
            None,
        )
        .await;
    }
    Ok(Json(
        serde_json::json!({ "kind": kind, "id": id, "restored": true }),
    ))
}

/// Rows that reference a purged parent, paired with the predicate selecting
/// them, **in the order they must be deleted**. Every identifier here is a
/// literal from this table, never anything from the request, so interpolating it
/// into SQL is safe.
///
/// `comments.project_id`, `ratings.project_id` and `forum_comments.post_id` are
/// declared without `ON DELETE`, and SQLite enforces foreign keys, so a parent
/// row cannot be removed while children exist. `forum_likes` has no foreign key
/// at all, but deleting its rows together with the target stops the likes table
/// from accumulating permanent orphans.
fn children_for(kind: &str) -> &'static [(&'static str, &'static str)] {
    match kind {
        "project" => &[
            ("comments", "project_id = ?1"),
            ("ratings", "project_id = ?1"),
        ],
        "forum_post" => &[
            // Must run while forum_comments still exists, hence the subquery.
            (
                "forum_likes",
                "target_kind = 'comment' AND target_id IN \
                 (SELECT id FROM forum_comments WHERE post_id = ?1)",
            ),
            ("forum_likes", "target_kind = 'post' AND target_id = ?1"),
            ("forum_comments", "post_id = ?1"),
        ],
        "forum_comment" => &[("forum_likes", "target_kind = 'comment' AND target_id = ?1")],
        _ => &[],
    }
}

/// Permanently remove a trashed item and everything that hangs off it. Admin
/// only. The existence check, the child deletions and the parent deletion share
/// one transaction, so a failure part-way through leaves the bin as it was.
pub async fn purge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, i64)>,
) -> Result<StatusCode, AppError> {
    let claims = require_role(&state, &headers, &[Role::Admin]).await?;
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::NotFound);
    }
    let table = table_for(&kind)?;

    let mut tx = state.pool.begin().await?;

    let trashed: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT id FROM {table} WHERE id = ?1 AND deleted_at IS NOT NULL"
    ))
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if trashed.is_none() {
        return Err(AppError::NotFound);
    }

    for (child_table, predicate) in children_for(&kind) {
        sqlx::query(&format!("DELETE FROM {child_table} WHERE {predicate}"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(&format!("DELETE FROM {table} WHERE id = ?1"))
        .bind(id)
        .execute(&mut *tx)
        .await?;

    // Written inside the same transaction: "an admin destroyed this row" must not
    // survive on its own if the delete rolls back, nor go missing if it commits.
    // Collect the file path before the row disappears, then delete it after commit.
    let stored_file: Option<String> = if kind == "attachment" {
        sqlx::query_scalar("SELECT storage_path FROM attachments WHERE id = ?1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
    } else {
        None
    };

    audit::record(
        &mut *tx,
        claims.sub,
        audit::TRASH_PURGE,
        &kind,
        Some(id),
        None,
    )
    .await?;

    tx.commit().await?;

    if let Some(path) = stored_file {
        crate::handlers::attachment::remove_stored_file(&state, &path).await;
    }
    Ok(StatusCode::NO_CONTENT)
}
