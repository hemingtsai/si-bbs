use axum::http::HeaderMap;

use crate::error::AppError;
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::{auth, cookies};

/// Find the access token: the `Authorization: Bearer` header first (the API and
/// every test use it), then the httpOnly cookie the browser client relies on.
///
/// The header wins when both are present, so a Bearer request is never affected by
/// whatever cookies the browser happens to hold.
pub fn access_token(headers: &HeaderMap) -> Option<String> {
    if let Some(raw) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        return raw.strip_prefix("Bearer ").map(str::to_string);
    }
    cookies::read(headers, cookies::ACCESS_COOKIE)
}

/// Verify the caller's token, then verify against the **database** that the account
/// still exists and is not banned, and return the claims with the *current* role
/// instead of the token's role.
///
/// This makes a ban or a role change effective immediately on every endpoint,
/// including the ones that only require a valid token. The cost is one indexed
/// lookup per request, which is negligible on SQLite.
pub async fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims, AppError> {
    let token = access_token(headers).ok_or(AppError::Unauthorized)?;
    let mut claims =
        auth::verify(&state.cfg, &token, "access").map_err(|_| AppError::Unauthorized)?;

    let privs = crate::models::user::current_privileges(&state.pool, claims.sub)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if privs.banned {
        return Err(AppError::Forbidden);
    }
    // A password change bumps the epoch, which invalidates every token issued
    // before it — including this one.
    if claims.tv != privs.token_version {
        return Err(AppError::Unauthorized);
    }
    claims.role = privs.role.as_str().to_string();
    Ok(claims)
}

/// Require the caller to hold one of `allowed` roles **according to the
/// database**. This is now a thin wrapper around [`require_auth`], which
/// already rejects banned and deleted accounts and refreshes the role.
pub async fn require_role(
    state: &AppState,
    headers: &HeaderMap,
    allowed: &[Role],
) -> Result<auth::Claims, AppError> {
    let claims = require_auth(state, headers).await?;
    let current_role = Role::parse(&claims.role).ok_or(AppError::Forbidden)?;
    if !allowed.contains(&current_role) {
        return Err(AppError::Forbidden);
    }
    Ok(claims)
}
