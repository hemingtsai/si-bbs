use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::AppError;
use crate::middleware::auth::require_role;
use crate::models::audit::AuditEntry;
use crate::models::page::Page;
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::audit;

#[derive(Debug, Deserialize, Default)]
pub struct UserListQuery {
    pub q: Option<String>,
    pub role: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AdminUser {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
    pub banned: i64,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct RoleInput {
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct BanInput {
    pub banned: bool,
}

/// Paginated member list with search and role filter. Admin only.
pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UserListQuery>,
) -> Result<Json<Page<AdminUser>>, AppError> {
    require_role(&state, &headers, &[Role::Admin]).await?;

    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let keyword =
        q.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"));
    // An unknown role must be rejected rather than silently ignored, otherwise
    // `?role=wizard` would return every user and look like a successful filter.
    let role =
        match q.role.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(raw) => Some(Role::parse(raw).ok_or_else(|| {
                AppError::BadRequest("role must be admin, moderator or user".into())
            })?),
        };

    let filters = "(?1 IS NULL OR username LIKE ?1 OR email LIKE ?1) \
                   AND (?2 IS NULL OR role = ?2)";

    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM users WHERE {filters}"))
        .bind(keyword.as_deref())
        .bind(role.map(Role::as_str))
        .fetch_one(&state.pool)
        .await?;

    let sql = format!(
        "SELECT id, username, email, role, banned, created_at FROM users WHERE {filters} \
         ORDER BY id ASC LIMIT ?3 OFFSET ?4"
    );
    let items: Vec<AdminUser> = sqlx::query_as(&sql)
        .bind(keyword.as_deref())
        .bind(role.map(Role::as_str))
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

/// Change a member's role. Admin only.
pub async fn set_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<RoleInput>,
) -> Result<Json<AdminUser>, AppError> {
    let caller = require_role(&state, &headers, &[Role::Admin]).await?;

    let new_role = Role::parse(input.role.trim())
        .ok_or_else(|| AppError::BadRequest("role must be admin, moderator or user".into()))?;

    let (old_role, banned): (String, i64) =
        sqlx::query_as("SELECT role, banned FROM users WHERE id = ?1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(AppError::NotFound)?;

    let old_role = Role::parse(&old_role).unwrap_or(Role::User);
    if old_role == Role::Admin && new_role != Role::Admin {
        // Never let the last usable admin demote themselves out of existence.
        let admins: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin' AND banned = 0")
                .fetch_one(&state.pool)
                .await?;
        if admins <= 1 {
            return Err(AppError::Conflict(
                "cannot demote the only remaining admin".into(),
            ));
        }
        if caller.sub == id {
            return Err(AppError::Conflict(
                "cannot demote yourself while other admins remain".into(),
            ));
        }
    }

    sqlx::query("UPDATE users SET role = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1")
        .bind(id)
        .bind(new_role.as_str())
        .execute(&state.pool)
        .await?;
    let _ = banned;

    audit::record_best_effort(
        &state.pool,
        caller.sub,
        audit::ROLE_CHANGE,
        "user",
        Some(id),
        Some(&format!("{} → {}", old_role.as_str(), new_role.as_str())),
    )
    .await;

    Ok(Json(fetch_user(&state, id).await?))
}

/// Ban or unban a member. Banned members cannot log in and lose access to
/// role-guarded endpoints immediately. Admin only, and never on yourself.
pub async fn set_ban(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<BanInput>,
) -> Result<Json<AdminUser>, AppError> {
    let caller = require_role(&state, &headers, &[Role::Admin]).await?;
    if caller.sub == id {
        return Err(AppError::Conflict("cannot ban yourself".into()));
    }

    let (role, banned): (String, i64) =
        sqlx::query_as("SELECT role, banned FROM users WHERE id = ?1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(AppError::NotFound)?;

    if input.banned && Role::parse(&role).unwrap_or(Role::User) == Role::Admin && banned == 0 {
        let admins: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin' AND banned = 0")
                .fetch_one(&state.pool)
                .await?;
        if admins <= 1 {
            return Err(AppError::Conflict(
                "cannot ban the only remaining admin".into(),
            ));
        }
    }

    sqlx::query("UPDATE users SET banned = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1")
        .bind(id)
        .bind(i64::from(input.banned))
        .execute(&state.pool)
        .await?;

    audit::record_best_effort(
        &state.pool,
        caller.sub,
        if input.banned {
            audit::USER_BAN
        } else {
            audit::USER_UNBAN
        },
        "user",
        Some(id),
        None,
    )
    .await;

    Ok(Json(fetch_user(&state, id).await?))
}

/// Site counters for the admin dashboard. Admin only.
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Stats>, AppError> {
    require_role(&state, &headers, &[Role::Admin]).await?;

    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;
    let banned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE banned = 1")
        .fetch_one(&state.pool)
        .await?;
    let projects: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE deleted_at IS NULL")
            .fetch_one(&state.pool)
            .await?;
    let projects_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE deleted_at IS NULL AND status = 'pending'",
    )
    .fetch_one(&state.pool)
    .await?;
    let projects_approved: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE deleted_at IS NULL AND status = 'approved'",
    )
    .fetch_one(&state.pool)
    .await?;
    let wiki: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM wiki_pages WHERE deleted_at IS NULL AND status = 'published'",
    )
    .fetch_one(&state.pool)
    .await?;
    let comments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE deleted_at IS NULL")
            .fetch_one(&state.pool)
            .await?;
    let ratings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ratings")
        .fetch_one(&state.pool)
        .await?;
    let trashed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM trash_view")
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(Stats {
        users,
        users_banned: banned,
        projects,
        projects_pending,
        projects_approved,
        wiki_published: wiki,
        comments,
        ratings,
        trashed,
    }))
}

/// Who did what to whom. Admin only.
///
/// `action` filters exactly, and the action names are the constants in
/// `services::audit`, so `/api/admin/audit?action=user.ban` is a precise query.
pub async fn list_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Page<AuditEntry>>, AppError> {
    require_role(&state, &headers, &[Role::Admin]).await?;

    let per_page = q.per_page.unwrap_or(50).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let action = q.action.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let filters = "(?1 IS NULL OR a.action = ?1)";

    let total: i64 =
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM audit_log a WHERE {filters}"))
            .bind(action)
            .fetch_one(&state.pool)
            .await?;

    let items: Vec<AuditEntry> = sqlx::query_as(&format!(
        "SELECT a.id, a.actor_id, u.username AS actor_username, a.action, a.target_kind, \
         a.target_id, a.detail, a.created_at \
         FROM audit_log a LEFT JOIN users u ON u.id = a.actor_id \
         WHERE {filters} ORDER BY a.created_at DESC, a.id DESC LIMIT ?2 OFFSET ?3"
    ))
    .bind(action)
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

#[derive(Debug, Deserialize, Default)]
pub struct AuditQuery {
    pub action: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub users: i64,
    pub users_banned: i64,
    pub projects: i64,
    pub projects_pending: i64,
    pub projects_approved: i64,
    pub wiki_published: i64,
    pub comments: i64,
    pub ratings: i64,
    pub trashed: i64,
}

async fn fetch_user(state: &AppState, id: i64) -> Result<AdminUser, AppError> {
    sqlx::query_as::<_, AdminUser>(
        "SELECT id, username, email, role, banned, created_at FROM users WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)
}
