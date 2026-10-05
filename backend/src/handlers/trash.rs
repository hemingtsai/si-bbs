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

/// The three soft-deletable entity kinds. `trash_view` unions exactly these.
const KINDS: [&str; 3] = ["wiki", "project", "comment"];

/// Map a public kind to its table. Whitelisted so the name can be interpolated
/// into SQL safely; SQLite cannot bind identifiers.
fn table_for(kind: &str) -> Result<&'static str, AppError> {
    match kind {
        "wiki" => Ok("wiki_pages"),
        "project" => Ok("projects"),
        "comment" => Ok("comments"),
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
    let claims = require_auth(&state.cfg, &headers)?;

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
    let claims = require_auth(&state.cfg, &headers)?;
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
    Ok(Json(
        serde_json::json!({ "kind": kind, "id": id, "restored": true }),
    ))
}

/// Permanently remove a trashed item. Admin only.
pub async fn purge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, i64)>,
) -> Result<StatusCode, AppError> {
    require_role(&state, &headers, &[Role::Admin]).await?;
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::NotFound);
    }
    let table = table_for(&kind)?;

    let sql = format!("DELETE FROM {table} WHERE id = ?1 AND deleted_at IS NOT NULL");
    let res = sqlx::query(&sql).bind(id).execute(&state.pool).await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
