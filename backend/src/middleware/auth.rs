use axum::http::HeaderMap;

use crate::error::AppError;
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::auth;

/// Extract and verify the `Authorization: Bearer <access>` header.
///
/// This is stateless: it trusts the signed claims and performs no database
/// access. Use [`require_role`] when the answer depends on the caller's current
/// privileges.
pub fn require_auth(
    cfg: &crate::config::Config,
    headers: &HeaderMap,
) -> Result<auth::Claims, AppError> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    let token = raw.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
    auth::verify(cfg, token, "access").map_err(|_| AppError::Unauthorized)
}

/// Require the caller to hold one of `allowed` roles **according to the
/// database**, not according to the token.
///
/// A role change or a ban therefore takes effect on the very next request
/// instead of waiting for the access token to expire. This costs one indexed
/// lookup, which is why it is only done for role-guarded endpoints.
pub async fn require_role(
    state: &AppState,
    headers: &HeaderMap,
    allowed: &[Role],
) -> Result<auth::Claims, AppError> {
    let claims = require_auth(&state.cfg, headers)?;
    let current = crate::models::user::current_privileges(&state.pool, claims.sub)
        .await?
        .ok_or(AppError::Unauthorized)?;

    if current.banned {
        return Err(AppError::Forbidden);
    }
    if !allowed.contains(&current.role) {
        return Err(AppError::Forbidden);
    }
    Ok(claims)
}
