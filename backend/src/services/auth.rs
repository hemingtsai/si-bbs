//! Password hashing (Argon2) and JWT issue/verify.
//!
//! Hashing is CPU-bound and runs inside `spawn_blocking` so it never stalls the
//! async runtime. JWT signing/verification is cheap enough to call inline.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::models::user::Role;

pub async fn hash_password(password: &str) -> Result<String, String> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub role: String,
    pub exp: usize,
    pub iat: usize,
    pub kind: String, // "access" | "refresh"
}

pub fn issue(
    cfg: &Config,
    user_id: i64,
    username: &str,
    role: Role,
    kind: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let ttl = match kind {
        "refresh" => Duration::seconds(cfg.refresh_ttl_secs),
        _ => Duration::seconds(cfg.access_ttl_secs),
    };
    let claims = Claims {
        sub: user_id,
        username: username.to_owned(),
        role: role.as_str().to_owned(),
        exp: (now + ttl).timestamp() as usize,
        iat: now.timestamp() as usize,
        kind: kind.to_owned(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()),
    )
}

pub fn issue_pair(
    cfg: &Config,
    user_id: i64,
    username: &str,
    role: Role,
) -> Result<(String, String), jsonwebtoken::errors::Error> {
    Ok((
        issue(cfg, user_id, username, role, "access")?,
        issue(cfg, user_id, username, role, "refresh")?,
    ))
}

pub fn verify(
    cfg: &Config,
    token: &str,
    expected_kind: &str,
) -> Result<Claims, jsonwebtoken::errors::Error> {
    let validation = Validation::default();
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()),
        &validation,
    )?;
    if data.claims.kind != expected_kind {
        return Err(jsonwebtoken::errors::ErrorKind::InvalidToken.into());
    }
    Ok(data.claims)
}
