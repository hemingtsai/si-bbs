use axum::http::HeaderMap;

use crate::config::Config;
use crate::error::AppError;
use crate::models::user::Role;
use crate::services::auth;

/// Extract and verify the `Authorization: Bearer <access>` header.
pub fn require_auth(cfg: &Config, headers: &HeaderMap) -> Result<auth::Claims, AppError> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    let token = raw.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
    auth::verify(cfg, token, "access").map_err(|_| AppError::Unauthorized)
}

/// Require the caller to hold one of the given roles.
pub fn require_role(
    cfg: &Config,
    headers: &HeaderMap,
    allowed: &[Role],
) -> Result<auth::Claims, AppError> {
    let claims = require_auth(cfg, headers)?;
    let role = Role::parse(&claims.role).ok_or(AppError::Forbidden)?;
    if allowed.contains(&role) {
        Ok(claims)
    } else {
        Err(AppError::Forbidden)
    }
}
