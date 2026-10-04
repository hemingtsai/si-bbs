use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Comment {
    pub id: i64,
    pub project_id: i64,
    pub user_id: i64,
    pub content: String,
    pub status: String,
    pub created_at: NaiveDateTime,
}

/// Comment joined with its author's username, as returned by the API.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct CommentOut {
    pub id: i64,
    pub project_id: i64,
    pub user_id: i64,
    pub username: String,
    pub content: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommentInput {
    pub content: String,
}
