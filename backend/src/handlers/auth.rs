use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::require_auth;
use crate::models::user::{Role, current_privileges};
use crate::routes::AppState;
use crate::services::auth;
use crate::services::validate;

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub email: String,
    pub password: String,
}

/// The register limiter is process-wide, so it needs a constant key.
const REGISTER_LIMIT_KEY: &str = "register";

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    // Shape checks first: they are free and must run before any hashing.
    let username = validate::username(&body.username).map_err(AppError::BadRequest)?;
    let email = validate::email(&body.email).map_err(AppError::BadRequest)?;
    validate::password(&body.password).map_err(AppError::BadRequest)?;

    if let Some(retry_after_secs) = state.register_limiter.retry_after(REGISTER_LIMIT_KEY) {
        return Err(AppError::TooManyRequests { retry_after_secs });
    }
    state.register_limiter.record(REGISTER_LIMIT_KEY);

    let hash = auth::hash_password(&body.password)
        .await
        .map_err(AppError::Internal)?;

    // `INSERT ... SELECT ... WHERE NOT EXISTS` keeps the case-insensitive
    // duplicate test and the insert in one statement, so two concurrent
    // requests cannot both pass the check. Plain UNIQUE sees `Alice` and
    // `alice` as different rows, which is how impersonation accounts appear.
    let res = sqlx::query(
        "INSERT INTO users (username, email, password_hash, role) \
         SELECT ?1, ?2, ?3, 'user' \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM users WHERE lower(username) = lower(?1) OR lower(email) = lower(?2) \
         )",
    )
    .bind(&username)
    .bind(&email)
    .bind(&hash)
    .execute(&state.pool)
    .await;

    match res {
        Ok(done) if done.rows_affected() == 1 => Ok((
            StatusCode::CREATED,
            Json(serde_json::json!({ "role": "user" })),
        )),
        Ok(_) => Err(AppError::Conflict("username or email taken".into())),
        // Defence in depth: a same-case duplicate still trips the column UNIQUE.
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
    // One bucket per account, case-folded so `Alice` and `alice` share a budget.
    let limit_key = body.username.trim().to_lowercase();
    if let Some(retry_after_secs) = state.login_limiter.retry_after(&limit_key) {
        return Err(AppError::TooManyRequests { retry_after_secs });
    }

    let row: Option<(i64, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, role, banned FROM users WHERE username = ?1",
    )
    .bind(body.username.trim())
    .fetch_optional(&state.pool)
    .await?;

    // Unknown account, wrong password and banned account all spend the same
    // budget: anything else lets an attacker tell the cases apart by watching
    // which one starts returning 429 first.
    let Some((id, username, hash, role_str, banned)) = row else {
        state.login_limiter.record(&limit_key);
        return Err(AppError::Unauthorized);
    };
    if !auth::verify_password(&body.password, &hash) {
        state.login_limiter.record(&limit_key);
        return Err(AppError::Unauthorized);
    }
    if banned != 0 {
        state.login_limiter.record(&limit_key);
        return Err(AppError::Forbidden);
    }

    state.login_limiter.clear(&limit_key);
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
    let claims = require_auth(&state, &headers).await?;

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
