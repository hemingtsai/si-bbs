//! Password-reset token lifecycle.
//!
//! Kept separate from the HTTP handler so the *real* token can be used in tests:
//! delivery (a log line, until a mailer exists) is the only untestable part.
//!
//! Tokens are stored as a SHA-256 hex digest. The lookup has to be by hash — an
//! Argon2 hash is salted, so the row could not be found from the presented token —
//! and a leaked database must not contain working reset links.

use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

/// How long a link stays valid.
pub const TTL_MINUTES: i64 = 30;

/// 32 random bytes, hex-encoded.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Deterministic hash of a token, hex-encoded.
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Mint a reset token for `user_id` and return the plaintext (the caller delivers
/// it; only the hash is persisted).
///
/// Any earlier unused token for the same account is invalidated, so requesting a
/// second link does not leave two working ones. Expired/used rows are pruned at the
/// same time.
pub async fn create(pool: &SqlitePool, user_id: i64) -> Result<String, sqlx::Error> {
    let token = generate_token();
    let hash = hash_token(&token);

    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE password_resets SET used_at = CURRENT_TIMESTAMP \
         WHERE user_id = ?1 AND used_at IS NULL",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO password_resets (user_id, token_hash, expires_at) \
         VALUES (?1, ?2, datetime('now', ?3))",
    )
    .bind(user_id)
    .bind(&hash)
    .bind(format!("+{TTL_MINUTES} minutes"))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM password_resets WHERE expires_at < datetime('now', '-1 day') \
         OR used_at < datetime('now', '-1 day')",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(token)
}

/// Spend a token and install `new_password_hash`, bumping the session epoch so
/// every existing token for that account dies.
///
/// `Ok(None)` means the link is invalid, expired or already used — the caller
/// answers 400 either way, because telling those apart helps nobody but an
/// attacker probing for valid links.
pub async fn consume(
    pool: &SqlitePool,
    token: &str,
    new_password_hash: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let hash = hash_token(token.trim());
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT id, user_id FROM password_resets \
         WHERE token_hash = ?1 AND used_at IS NULL AND expires_at > datetime('now')",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await?;
    let Some((reset_id, user_id)) = row else {
        return Ok(None);
    };

    let mut tx = pool.begin().await?;
    // Claim first: if two requests race, only one sees a row affected.
    let claimed = sqlx::query(
        "UPDATE password_resets SET used_at = CURRENT_TIMESTAMP \
         WHERE id = ?1 AND used_at IS NULL",
    )
    .bind(reset_id)
    .execute(&mut *tx)
    .await?;
    if claimed.rows_affected() == 0 {
        return Ok(None);
    }
    sqlx::query(
        "UPDATE users SET password_hash = ?2, token_version = token_version + 1, \
         updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
    )
    .bind(user_id)
    .bind(new_password_hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Some(user_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_random_and_hash_deterministically() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b, "two tokens must not collide");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_eq!(hash_token(&a).len(), 64);
        assert_ne!(hash_token(&a), hash_token(&b));
        // The hash is not the token.
        assert_ne!(hash_token(&a), a);
    }
}
