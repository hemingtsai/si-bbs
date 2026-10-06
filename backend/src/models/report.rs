use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// The reportable entity kinds. Also the whitelist for the `target_kind` field:
/// the kind is interpolated into a lookup, so it can never come from the request
/// unchecked.
pub const TARGET_KINDS: [&str; 5] = ["forum_post", "forum_comment", "wiki", "project", "comment"];

/// A report as staff sees it, with enough of the target to triage without a second
/// request.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Report {
    pub id: i64,
    pub reporter_id: i64,
    pub reporter_username: Option<String>,
    pub target_kind: String,
    pub target_id: i64,
    pub reason: String,
    pub status: String,
    pub handled_by: Option<i64>,
    pub handled_by_username: Option<String>,
    pub handled_at: Option<NaiveDateTime>,
    pub note: Option<String>,
    pub created_at: NaiveDateTime,
    /// Title (or a snippet) of the reported thing, resolved per kind.
    pub target_title: Option<String>,
    /// Whether the target is already soft-deleted, so staff can skip resolved cases.
    pub target_deleted: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportInput {
    pub target_kind: String,
    pub target_id: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolveInput {
    /// `resolved` (acted on) or `dismissed` (nothing wrong).
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReportListQuery {
    pub status: Option<String>,
    pub kind: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}
