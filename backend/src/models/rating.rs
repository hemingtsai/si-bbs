use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Rating {
    pub id: i64,
    pub project_id: i64,
    pub user_id: i64,
    pub score: i64,
    pub comment: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateInput {
    pub score: i64,
    #[serde(default)]
    pub comment: Option<String>,
}

/// Average score and vote count for one project.
#[derive(Debug, Clone, Serialize)]
pub struct RatingSummary {
    pub project_id: i64,
    pub average: f64,
    pub count: i64,
}

/// Result of an upsert: the refreshed aggregate plus the caller's own score.
#[derive(Debug, Clone, Serialize)]
pub struct RateAck {
    #[serde(flatten)]
    pub summary: RatingSummary,
    pub my_score: i64,
}
