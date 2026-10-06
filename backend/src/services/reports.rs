//! Report bookkeeping helpers shared with the delete paths.
//!
//! Closing the loop matters: if a moderator removes the reported content directly,
//! the report must not stay in the queue for someone else to handle.

use sqlx::SqlitePool;

/// Mark every open report for one target as resolved, because the content it
/// pointed at is gone. Best effort — a failure here must not undo a deletion.
pub async fn resolve_for_target(
    pool: &SqlitePool,
    target_kind: &str,
    target_id: i64,
    handled_by: i64,
    note: &str,
) {
    let res = sqlx::query(
        "UPDATE content_reports SET status = 'resolved', handled_by = ?3, \
         handled_at = CURRENT_TIMESTAMP, note = ?4 \
         WHERE target_kind = ?1 AND target_id = ?2 AND status = 'open'",
    )
    .bind(target_kind)
    .bind(target_id)
    .bind(handled_by)
    .bind(note)
    .execute(pool)
    .await;

    match res {
        Ok(done) if done.rows_affected() > 0 => tracing::info!(
            target_kind,
            target_id,
            resolved = done.rows_affected(),
            "open reports closed with the content"
        ),
        Ok(_) => {}
        Err(err) => tracing::error!(error = %err, target_kind, target_id, "closing reports failed"),
    }
}
