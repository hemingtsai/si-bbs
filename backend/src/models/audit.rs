use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

/// One privileged action, as returned by the admin audit endpoint.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub actor_id: Option<i64>,
    pub actor_username: Option<String>,
    pub action: String,
    pub target_kind: String,
    pub target_id: Option<i64>,
    pub detail: Option<String>,
    pub created_at: NaiveDateTime,
}
