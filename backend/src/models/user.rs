use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Moderator,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Moderator => "moderator",
            Self::User => "user",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Self::Admin),
            "moderator" => Some(Self::Moderator),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    #[serde(skip)]
    pub password_hash: String,
    pub role: String,
    pub banned: i64,
    pub created_at: chrono::NaiveDateTime,
}

impl User {
    pub fn role_enum(&self) -> Role {
        Role::parse(&self.role).unwrap_or(Role::User)
    }

    pub fn is_banned(&self) -> bool {
        self.banned != 0
    }
}

/// Authoritative privilege state, read straight from the database so that role
/// changes and bans apply immediately instead of at token expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Privileges {
    pub role: Role,
    pub banned: bool,
    /// Session epoch; see `services::auth::Claims::tv`.
    pub token_version: i64,
}

/// `None` when the user no longer exists.
pub async fn current_privileges(
    pool: &sqlx::SqlitePool,
    user_id: i64,
) -> Result<Option<Privileges>, crate::error::AppError> {
    let row: Option<(String, i64, i64)> =
        sqlx::query_as("SELECT role, banned, token_version FROM users WHERE id = ?1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(role, banned, token_version)| Privileges {
        // An unrecognised role string must never widen access.
        role: Role::parse(&role).unwrap_or(Role::User),
        banned: banned != 0,
        token_version,
    }))
}
