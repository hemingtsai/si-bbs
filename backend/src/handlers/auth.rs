use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::require_auth;
use crate::models::user::Role;
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
) -> Result<Json<serde_json::Value>, AppError> {
    if body.username.trim().is_empty() || body.email.trim().is_empty() {
        return Err(AppError::BadRequest("username/email required".into()));
    }
    if body.password.len() < 6 {
        return Err(AppError::BadRequest("password too short".into()));
    }
    let hash = auth::hash_password(&body.password)
        .await
        .map_err(|e| AppError::Internal(e))?;
    let res = sqlx::query(
        "INSERT INTO users (username, email, password_hash, role) VALUES (?1, ?2, ?3, 'user')",
    )
    .bind(&body.username)
    .bind(&body.email)
    .bind(&hash)
    .execute(&state.pool)
    .await;
    match res {
        Ok(_) => Ok(Json(serde_json::json!({ "role": "user" }))),
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
    let row: Option<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, username, password_hash, role FROM users WHERE username = ?1",
    )
    .bind(&body.username)
    .fetch_optional(&state.pool)
    .await?;
    let (id, username, hash, role_str) = row.ok_or(AppError::Unauthorized)?;
    if !auth::verify_password(&body.password, &hash) {
        return Err(AppError::Unauthorized);
    }
    let role = Role::from_str(&role_str).unwrap_or(Role::User);
    let (access, refresh) = auth::issue_pair(&state.cfg, id, &username, role)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "access_token": access,
        "refresh_token": refresh,
        "role": role.as_str(),
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
    let role = Role::from_str(&claims.role).unwrap_or(Role::User);
    let (access, refresh) = auth::issue_pair(&state.cfg, claims.sub, &claims.username, role)
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
    Ok(Json(serde_json::json!({
        "id": claims.sub,
        "username": claims.username,
        "role": claims.role,
    })))
}