use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::require_auth;
use crate::models::user::{Role, current_privileges};
use crate::routes::AppState;
use crate::services::auth;

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub email: String,
    pub password: String,
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if body.username.trim().is_empty() || body.email.trim().is_empty() {
        return Err(AppError::BadRequest("username/email required".into()));
    }
    if body.password.len() < 6 {
        return Err(AppError::BadRequest("password too short".into()));
    }
    let hash = auth::hash_password(&body.password)
        .await
        .map_err(AppError::Internal)?;
    let res = sqlx::query(
        "INSERT INTO users (username, email, password_hash, role) VALUES (?1, ?2, ?3, 'user')",
    )
    .bind(&body.username)
    .bind(&body.email)
    .bind(&hash)
    .execute(&state.pool)
    .await;
    match res {
        Ok(_) => Ok((
            StatusCode::CREATED,
            Json(serde_json::json!({ "role": "user" })),
        )),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            Err(AppError::Conflict("username or email taken".into()))
        }
        Err(e) => Err(AppError::Internal(e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row: Option<(i64, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, role, banned FROM users WHERE username = ?1",
    )
    .bind(&body.username)
    .fetch_optional(&state.pool)
    .await?;
    let (id, username, hash, role_str, banned) = row.ok_or(AppError::Unauthorized)?;
    if !auth::verify_password(&body.password, &hash) {
        return Err(AppError::Unauthorized);
    }
    // Checked after the password so a wrong password still returns 401 rather
    // than revealing that the account exists but is banned.
    if banned != 0 {
        return Err(AppError::Forbidden);
    }
    let role = Role::parse(&role_str).unwrap_or(Role::User);
    let (access, refresh) = auth::issue_pair(&state.cfg, id, &username, role)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "access_token": access,
        "refresh_token": refresh,
        "role": role.as_str(),
        "user_id": id,
        "username": username,
    })))
}

#[derive(Deserialize)]
pub struct RefreshReq {
    pub refresh_token: String,
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<RefreshReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = auth::verify(&state.cfg, &body.refresh_token, "refresh")
        .map_err(|_| AppError::Unauthorized)?;

    // The role is re-read from the database instead of being copied out of the
    // refresh token. Without this a demoted admin could keep minting admin
    // access tokens forever.
    let privs = current_privileges(&state.pool, claims.sub)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if privs.banned {
        return Err(AppError::Forbidden);
    }
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?1")
        .bind(claims.sub)
        .fetch_one(&state.pool)
        .await?;

    let (access, refresh) = auth::issue_pair(&state.cfg, claims.sub, &username, privs.role)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "access_token": access,
        "refresh_token": refresh,
    })))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_auth(&state.cfg, &headers)?;

    // Read the current row: a token minted before a role change or ban must not
    // report stale privileges.
    let row: Option<(String, String, i64)> =
        sqlx::query_as("SELECT username, role, banned FROM users WHERE id = ?1")
            .bind(claims.sub)
            .fetch_optional(&state.pool)
            .await?;
    let (username, role, banned) = row.ok_or(AppError::Unauthorized)?;
    if banned != 0 {
        return Err(AppError::Forbidden);
    }

    Ok(Json(serde_json::json!({
        "id": claims.sub,
        "username": username,
        "role": Role::parse(&role).unwrap_or(Role::User).as_str(),
        "banned": banned != 0,
    })))
}
