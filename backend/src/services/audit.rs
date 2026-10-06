//! Audit trail for privileged actions.
//!
//! Every write here is *best effort*: a failing audit insert must never undo a
//! successful action, but it must be loud, so failures are logged at ERROR. The
//! one place that needs the record to be atomic with the action (purging a trashed
//! item) passes the open transaction as the executor instead.

use sqlx::SqlitePool;

/// Action names. Kept as constants so the query filter and the writers cannot
/// drift apart, and so the set is greppable.
pub const ROLE_CHANGE: &str = "user.role_change";
pub const USER_BAN: &str = "user.ban";
pub const USER_UNBAN: &str = "user.unban";
pub const PROJECT_REVIEW: &str = "project.review";
pub const FORUM_FEATURE: &str = "forum.feature";
pub const FORUM_RULE_UPDATE: &str = "forum.rule_update";
pub const TRASH_PURGE: &str = "trash.purge";
pub const TRASH_RESTORE: &str = "trash.restore";
pub const REPORT_RESOLVE: &str = "report.resolve";

/// Insert one entry, using any executor (the pool, or a transaction in progress).
pub async fn record<'e, E>(
    executor: E,
    actor_id: i64,
    action: &str,
    target_kind: &str,
    target_id: Option<i64>,
    detail: Option<&str>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO audit_log (actor_id, action, target_kind, target_id, detail) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(actor_id)
    .bind(action)
    .bind(target_kind)
    .bind(target_id)
    .bind(detail)
    .execute(executor)
    .await
    .map(|_| ())
}

/// [`record`] against the pool, swallowing (but logging) failures.
pub async fn record_best_effort(
    pool: &SqlitePool,
    actor_id: i64,
    action: &str,
    target_kind: &str,
    target_id: Option<i64>,
    detail: Option<&str>,
) {
    if let Err(err) = record(pool, actor_id, action, target_kind, target_id, detail).await {
        tracing::error!(error = %err, action, target_kind, target_id, "audit record failed");
    }
}
