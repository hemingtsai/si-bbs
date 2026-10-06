use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::require_auth;
use crate::models::user::{Role, current_privileges};
use crate::routes::AppState;
use crate::services::auth;
use crate::services::{cookies, password_reset, validate};

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
) -> Result<impl axum::response::IntoResponse, AppError> {
    // One bucket per account, case-folded so `Alice` and `alice` share a budget.
    let limit_key = body.username.trim().to_lowercase();
    if let Some(retry_after_secs) = state.login_limiter.retry_after(&limit_key) {
        return Err(AppError::TooManyRequests { retry_after_secs });
    }

    let row: Option<(i64, String, String, String, i64, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, role, banned, token_version \
         FROM users WHERE username = ?1",
    )
    .bind(body.username.trim())
    .fetch_optional(&state.pool)
    .await?;

    // Unknown account, wrong password and banned account all spend the same
    // budget: anything else lets an attacker tell the cases apart by watching
    // which one starts returning 429 first.
    let Some((id, username, hash, role_str, banned, token_version)) = row else {
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
    let (access, refresh) = auth::issue_pair(&state.cfg, id, &username, role, token_version)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok((
        session_cookie_jar(&state, &access, &refresh),
        Json(serde_json::json!({
            "access_token": access,
            "refresh_token": refresh,
            "role": role.as_str(),
            "user_id": id,
            "username": username,
        })),
    ))
}

/// The `Set-Cookie` headers for a browser session, plus a fresh CSRF token.
///
/// The JSON body still carries the tokens: the API and the whole test suite use
/// `Authorization: Bearer`, and only the frontend uses the cookies.
fn session_cookie_jar(
    state: &AppState,
    access: &str,
    refresh: &str,
) -> axum::response::AppendHeaders<[(axum::http::HeaderName, String); 3]> {
    let cookies = cookies::session_cookies(
        access,
        refresh,
        &cookies::new_csrf_token(),
        state.cfg.access_ttl_secs,
        state.cfg.refresh_ttl_secs,
        state.cfg.cookie_secure,
    );
    axum::response::AppendHeaders([
        (axum::http::header::SET_COOKIE, cookies[0].clone()),
        (axum::http::header::SET_COOKIE, cookies[1].clone()),
        (axum::http::header::SET_COOKIE, cookies[2].clone()),
    ])
}

#[derive(Deserialize, Default)]
pub struct RefreshReq {
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// Exchange a refresh token for a new pair.
///
/// The token may arrive in the body (API clients) or in the httpOnly refresh cookie
/// (browser). Rotation also refreshes the cookies, so a browser session keeps
/// sliding without JavaScript ever seeing a token.
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<RefreshReq>>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let from_body = body.and_then(|Json(req)| req.refresh_token);
    let presented = from_body
        .or_else(|| cookies::read(&headers, cookies::REFRESH_COOKIE))
        .ok_or(AppError::Unauthorized)?;
    let claims =
        auth::verify(&state.cfg, &presented, "refresh").map_err(|_| AppError::Unauthorized)?;

    // The role is re-read from the database instead of being copied out of the
    // refresh token. Without this a demoted admin could keep minting admin
    // access tokens forever.
    let privs = current_privileges(&state.pool, claims.sub)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if privs.banned {
        return Err(AppError::Forbidden);
    }
    // Same epoch check as `require_auth`: a password change must not be
    // circumventable by refreshing an older token.
    if claims.tv != privs.token_version {
        return Err(AppError::Unauthorized);
    }
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?1")
        .bind(claims.sub)
        .fetch_one(&state.pool)
        .await?;

    let (access, refresh) = auth::issue_pair(
        &state.cfg,
        claims.sub,
        &username,
        privs.role,
        privs.token_version,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok((
        session_cookie_jar(&state, &access, &refresh),
        Json(serde_json::json!({
            "access_token": access,
            "refresh_token": refresh,
        })),
    ))
}

/// Drop the session cookies. The JSON tokens are the client's to forget, so this only
/// has to clear what the server set.
pub async fn logout(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let cleared = cookies::clear_cookies(state.cfg.cookie_secure);
    axum::response::AppendHeaders([
        (axum::http::header::SET_COOKIE, cleared[0].clone()),
        (axum::http::header::SET_COOKIE, cleared[1].clone()),
        (axum::http::header::SET_COOKIE, cleared[2].clone()),
    ])
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_auth(&state, &headers).await?;

    // Read the current row: a token minted before a role change or ban must not
    // report stale privileges.
    let row: Option<(String, String, i64, String)> =
        sqlx::query_as("SELECT username, role, banned, email FROM users WHERE id = ?1")
            .bind(claims.sub)
            .fetch_optional(&state.pool)
            .await?;
    let (username, role, banned, email) = row.ok_or(AppError::Unauthorized)?;
    if banned != 0 {
        return Err(AppError::Forbidden);
    }

    Ok(Json(serde_json::json!({
        "id": claims.sub,
        "username": username,
        "email": email,
        "role": Role::parse(&role).unwrap_or(Role::User).as_str(),
        "banned": banned != 0,
    })))
}

#[derive(Deserialize)]
pub struct PasswordChangeReq {
    pub current_password: String,
    pub new_password: String,
}

/// Change the caller's own password.
///
/// Bumps `users.token_version`, which invalidates every token issued earlier —
/// including the caller's own. That is the point (a stolen session must not
/// survive the password change), so a fresh pair is returned for the caller to
/// keep working, and the frontend swaps it in.
pub async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PasswordChangeReq>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let claims = require_auth(&state, &headers).await?;
    validate::password(&body.new_password).map_err(AppError::BadRequest)?;
    if body.new_password == body.current_password {
        return Err(AppError::BadRequest(
            "the new password must differ from the current one".into(),
        ));
    }

    let row: Option<(String, String, i64)> =
        sqlx::query_as("SELECT password_hash, role, token_version FROM users WHERE id = ?1")
            .bind(claims.sub)
            .fetch_optional(&state.pool)
            .await?;
    let (hash, role_str, token_version) = row.ok_or(AppError::Unauthorized)?;
    if !auth::verify_password(&body.current_password, &hash) {
        // Same budget as a failed login: verifying a password here is just as
        // useful to an attacker holding a stolen token.
        let key = format!("pwchange:{}", claims.sub);
        if let Some(retry_after_secs) = state.login_limiter.retry_after(&key) {
            return Err(AppError::TooManyRequests { retry_after_secs });
        }
        state.login_limiter.record(&key);
        return Err(AppError::Unauthorized);
    }
    state
        .login_limiter
        .clear(&format!("pwchange:{}", claims.sub));

    let new_hash = auth::hash_password(&body.new_password)
        .await
        .map_err(AppError::Internal)?;
    let next_version = token_version + 1;
    sqlx::query(
        "UPDATE users SET password_hash = ?2, token_version = ?3, \
         updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
    )
    .bind(claims.sub)
    .bind(&new_hash)
    .bind(next_version)
    .execute(&state.pool)
    .await?;

    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?1")
        .bind(claims.sub)
        .fetch_one(&state.pool)
        .await?;
    let role = Role::parse(&role_str).unwrap_or(Role::User);
    let (access, refresh) = auth::issue_pair(&state.cfg, claims.sub, &username, role, next_version)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!(
        user_id = claims.sub,
        "password changed; older tokens invalidated"
    );
    // The cookies have to be reissued too: the tokens the browser holds were just
    // invalidated, and a browser-only client has no other way to learn the new pair.
    Ok((
        session_cookie_jar(&state, &access, &refresh),
        Json(serde_json::json!({
            "access_token": access,
            "refresh_token": refresh,
            "role": role.as_str(),
            "user_id": claims.sub,
            "username": username,
        })),
    ))
}

#[derive(Deserialize)]
pub struct EmailChangeReq {
    pub password: String,
    pub new_email: String,
}

/// Change the caller's own email address. Password confirmation is required: an
/// unattended session must not be able to move the account's contact address.
pub async fn change_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmailChangeReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let email = validate::email(&body.new_email).map_err(AppError::BadRequest)?;

    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?1")
        .bind(claims.sub)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if !auth::verify_password(&body.password, &hash) {
        let key = format!("emailchange:{}", claims.sub);
        if let Some(retry_after_secs) = state.login_limiter.retry_after(&key) {
            return Err(AppError::TooManyRequests { retry_after_secs });
        }
        state.login_limiter.record(&key);
        return Err(AppError::Unauthorized);
    }
    state
        .login_limiter
        .clear(&format!("emailchange:{}", claims.sub));

    // Same case-insensitive uniqueness rule as registration, enforced in one
    // statement so two accounts cannot race into the same address.
    let res = sqlx::query(
        "UPDATE users SET email = ?2, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ?1 \
           AND NOT EXISTS (SELECT 1 FROM users WHERE lower(email) = lower(?2) AND id != ?1)",
    )
    .bind(claims.sub)
    .bind(&email)
    .execute(&state.pool)
    .await?;

    if res.rows_affected() == 0 {
        // Either the address is taken, or the row vanished; both are conflicts the
        // caller can act on.
        return Err(AppError::Conflict("email is already in use".into()));
    }

    Ok(Json(serde_json::json!({ "email": email })))
}

#[derive(Deserialize)]
pub struct ProfileInput {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

/// Update the caller's own profile. Every field is optional; an omitted field is
/// left alone, while an empty string clears it.
pub async fn update_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProfileInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_auth(&state, &headers).await?;

    // `None` = "not mentioned", `Some("")` = "clear it".
    let display_name = match input.display_name.as_deref() {
        None => None,
        Some(raw) => Some(Some(
            validate::display_name(raw)
                .map_err(AppError::BadRequest)?
                .clone(),
        )),
    };
    let bio = match input.bio.as_deref() {
        None => None,
        Some(raw) => Some(validate::bio(raw).map_err(AppError::BadRequest)?),
    };
    let avatar_url = match input.avatar_url.as_deref() {
        None => None,
        Some(raw) => Some(validate::avatar_url(raw).map_err(AppError::BadRequest)?),
    };

    // COALESCE keeps the ones that were not sent; empty strings are turned into
    // NULL so "no display name" has a single representation.
    sqlx::query(
        "UPDATE users SET \
         display_name = CASE WHEN ?2 IS NULL THEN display_name \
                             WHEN ?2 = '' THEN NULL ELSE ?2 END, \
         bio = CASE WHEN ?3 IS NULL THEN bio \
                    WHEN ?3 = '' THEN NULL ELSE ?3 END, \
         avatar_url = CASE WHEN ?4 IS NULL THEN avatar_url \
                           WHEN ?4 = '' THEN NULL ELSE ?4 END, \
         updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
    )
    .bind(claims.sub)
    .bind(display_name.flatten().as_deref())
    .bind(bio.as_deref())
    .bind(avatar_url.as_deref())
    .execute(&state.pool)
    .await?;

    Ok(Json(profile_of(&state, claims.sub).await?))
}

/// The caller's own profile row.
pub async fn profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    Ok(Json(profile_of(&state, claims.sub).await?))
}

async fn profile_of(state: &AppState, user_id: i64) -> Result<serde_json::Value, AppError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: i64,
        username: String,
        email: String,
        display_name: Option<String>,
        bio: Option<String>,
        avatar_url: Option<String>,
        role: String,
        banned: i64,
        created_at: chrono::NaiveDateTime,
    }

    let row: Row = sqlx::query_as(
        "SELECT id, username, email, display_name, bio, avatar_url, role, banned, created_at \
         FROM users WHERE id = ?1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::Unauthorized)?;

    Ok(serde_json::json!({
        "id": row.id,
        "username": row.username,
        "display_name": row.display_name,
        "email": row.email,
        "bio": row.bio,
        "avatar_url": row.avatar_url,
        "role": Role::parse(&row.role).unwrap_or(Role::User).as_str(),
        "banned": row.banned != 0,
        "created_at": row.created_at,
    }))
}

#[derive(Deserialize)]
pub struct ForgotReq {
    pub email: String,
}

/// Request a password reset link.
///
/// Always answers 202 with the same body, whether or not the address exists:
/// otherwise this endpoint becomes an account-existence oracle. Delivery is the
/// missing piece — see the note in `docs/deployment.md`; without a mailer the link
/// is written to the server log, which is why the response never contains it.
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(body): Json<ForgotReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let generic = (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "if the address exists, a reset link has been issued"
        })),
    );

    let email = body.email.trim().to_lowercase();
    if email.is_empty() {
        return Ok(generic);
    }
    // Per-address and process-wide budgets: this endpoint hashes and writes.
    let key = format!("forgot:{email}");
    if state.register_limiter.retry_after(&key).is_some()
        || state
            .register_limiter
            .retry_after("forgot:global")
            .is_some()
    {
        // Still generic: a 429 here would reveal that the address is interesting.
        tracing::warn!(%email, "password reset request throttled");
        return Ok(generic);
    }
    state.register_limiter.record(&key);
    state.register_limiter.record("forgot:global");

    let user: Option<i64> =
        sqlx::query_scalar("SELECT id FROM users WHERE lower(email) = ?1 AND banned = 0")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await?;

    let Some(user_id) = user else {
        // Same shape, no work.
        return Ok(generic);
    };

    let token = password_reset::create(&state.pool, user_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let base = crate::services::feed::base_url(&state.cfg.public_base_url, None, None);
    // Loud on purpose: with no mailer configured this log line *is* the delivery
    // channel, and docs/deployment.md says so.
    tracing::warn!(
        user_id,
        "password reset link (no mailer configured): {base}/reset?token={token}"
    );

    Ok(generic)
}

#[derive(Deserialize)]
pub struct ResetReq {
    pub token: String,
    pub new_password: String,
}

/// Complete a reset with the token from the reset link.
pub async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate::password(&body.new_password).map_err(AppError::BadRequest)?;
    if body.token.trim().is_empty() {
        return Err(AppError::BadRequest("token is required".into()));
    }

    let new_hash = auth::hash_password(&body.new_password)
        .await
        .map_err(AppError::Internal)?;

    let user_id = password_reset::consume(&state.pool, &body.token, &new_hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::BadRequest("this reset link is invalid or has expired".into()))?;

    tracing::info!(
        user_id,
        "password reset completed; all sessions invalidated"
    );
    Ok(Json(serde_json::json!({ "status": "password updated" })))
}
