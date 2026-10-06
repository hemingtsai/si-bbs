//! Reporting content and working the resulting queue.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::error::AppError;
use crate::middleware::auth::{require_auth, require_role};
use crate::models::page::Page;
use crate::models::report::{Report, ReportInput, ReportListQuery, ResolveInput, TARGET_KINDS};
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::audit;

const MAX_REASON_LEN: usize = 500;

/// Report a post, comment, wiki page or project. Login required.
///
/// The target must exist and be live: reporting something already deleted would
/// only create busywork (`reports::resolve_for_target` closes a report when the
/// content goes away).
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReportInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let claims = require_auth(&state, &headers).await?;

    let kind = input.target_kind.trim();
    if !TARGET_KINDS.contains(&kind) {
        return Err(AppError::BadRequest(
            "target_kind must be forum_post, forum_comment, wiki, project or comment".into(),
        ));
    }
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(AppError::BadRequest("reason is required".into()));
    }
    if reason.chars().count() > MAX_REASON_LEN {
        return Err(AppError::BadRequest(format!(
            "reason cannot exceed {MAX_REASON_LEN} characters"
        )));
    }

    // Per-account budget: a single user must not be able to fill the queue.
    let limit_key = format!("report:{}", claims.sub);
    if let Some(retry_after_secs) = state.report_limiter.retry_after(&limit_key) {
        return Err(AppError::TooManyRequests { retry_after_secs });
    }

    if !target_is_live(&state, kind, input.target_id).await? {
        return Err(AppError::NotFound);
    }
    state.report_limiter.record(&limit_key);

    let res = sqlx::query(
        "INSERT INTO content_reports (reporter_id, target_kind, target_id, reason) \
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(claims.sub)
    .bind(kind)
    .bind(input.target_id)
    .bind(reason)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => Ok((
            StatusCode::CREATED,
            Json(serde_json::json!({ "status": "open" })),
        )),
        // The UNIQUE(reporter, target) constraint: reporting twice is a no-op, not
        // an error the caller has to understand.
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "status": "already reported" })),
        )),
        Err(e) => Err(AppError::Internal(e.to_string())),
    }
}

/// The queue. Moderators and admins only; `status=open` is the default view.
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ReportListQuery>,
) -> Result<Json<Page<Report>>, AppError> {
    require_role(&state, &headers, &[Role::Admin, Role::Moderator]).await?;

    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    // Default to the things that still need attention.
    let status = match q.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Some("open".to_string()),
        Some("all") => None,
        Some(other) => Some(
            match other {
                "open" | "resolved" | "dismissed" => other,
                _ => {
                    return Err(AppError::BadRequest(
                        "status must be open, resolved, dismissed or all".into(),
                    ));
                }
            }
            .to_string(),
        ),
    };
    let kind = match q.kind.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(raw) if TARGET_KINDS.contains(&raw) => Some(raw.to_string()),
        Some(_) => {
            return Err(AppError::BadRequest(
                "kind must be forum_post, forum_comment, wiki, project or comment".into(),
            ));
        }
    };

    let filters = "(?1 IS NULL OR r.status = ?1) AND (?2 IS NULL OR r.target_kind = ?2)";

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM content_reports r WHERE {filters}"
    ))
    .bind(status.as_deref())
    .bind(kind.as_deref())
    .fetch_one(&state.pool)
    .await?;

    let items: Vec<Report> = sqlx::query_as(&format!(
        "{REPORT_SELECT} WHERE {filters} \
         ORDER BY r.created_at ASC, r.id ASC LIMIT ?3 OFFSET ?4"
    ))
    .bind(status.as_deref())
    .bind(kind.as_deref())
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(Page {
        items,
        total,
        page,
        per_page,
    }))
}

/// Close a report: `resolved` (acted on) or `dismissed` (nothing wrong).
pub async fn resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<ResolveInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = require_role(&state, &headers, &[Role::Admin, Role::Moderator]).await?;

    let status = match input.status.trim() {
        "resolved" => "resolved",
        "dismissed" => "dismissed",
        _ => {
            return Err(AppError::BadRequest(
                "status must be resolved or dismissed".into(),
            ));
        }
    };
    let note = input
        .note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if note
        .as_ref()
        .is_some_and(|n| n.chars().count() > MAX_REASON_LEN)
    {
        return Err(AppError::BadRequest(format!(
            "note cannot exceed {MAX_REASON_LEN} characters"
        )));
    }

    let res = sqlx::query(
        "UPDATE content_reports SET status = ?2, handled_by = ?3, \
         handled_at = CURRENT_TIMESTAMP, note = ?4 WHERE id = ?1 AND status = 'open'",
    )
    .bind(id)
    .bind(status)
    .bind(claims.sub)
    .bind(note.as_deref())
    .execute(&state.pool)
    .await?;

    // Already handled (or gone): report that instead of pretending it worked.
    if res.rows_affected() == 0 {
        let exists: Option<String> =
            sqlx::query_scalar("SELECT status FROM content_reports WHERE id = ?1")
                .bind(id)
                .fetch_optional(&state.pool)
                .await?;
        return match exists {
            None => Err(AppError::NotFound),
            Some(current) => Err(AppError::Conflict(format!("report is already {current}"))),
        };
    }

    // The decision *and* the reason for it: "resolved" alone does not tell a later
    // reader why, and the note is the substance of the decision. Bounded so a long
    // note cannot bloat the log.
    let detail = match note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(note) => format!("{status}: {}", note.chars().take(200).collect::<String>()),
        None => status.to_string(),
    };
    audit::record_best_effort(
        &state.pool,
        claims.sub,
        audit::REPORT_RESOLVE,
        "report",
        Some(id),
        Some(&detail),
    )
    .await;

    Ok(Json(serde_json::json!({ "id": id, "status": status })))
}

/// Reports the caller filed, so they can see what happened to them.
pub async fn mine(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ReportListQuery>,
) -> Result<Json<Page<Report>>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);

    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM content_reports WHERE reporter_id = ?1")
            .bind(claims.sub)
            .fetch_one(&state.pool)
            .await?;

    let items: Vec<Report> = sqlx::query_as(&format!(
        "{REPORT_SELECT} WHERE r.reporter_id = ?1 ORDER BY r.created_at DESC, r.id DESC \
         LIMIT ?2 OFFSET ?3"
    ))
    .bind(claims.sub)
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(Page {
        items,
        total,
        page,
        per_page,
    }))
}

/// Selects a report plus the reporter/handler names and enough of the target to
/// triage it in one round trip. The per-kind subqueries are safe because
/// `target_kind` is constrained by a CHECK and by [`TARGET_KINDS`].
const REPORT_SELECT: &str = "SELECT r.id, r.reporter_id, ru.username AS reporter_username, \
     r.target_kind, r.target_id, r.reason, r.status, r.handled_by, \
     hu.username AS handled_by_username, r.handled_at, r.note, r.created_at, \
     CASE r.target_kind \
       WHEN 'forum_post' THEN (SELECT title FROM forum_posts WHERE id = r.target_id) \
       WHEN 'forum_comment' THEN (SELECT substr(content, 1, 80) FROM forum_comments WHERE id = r.target_id) \
       WHEN 'wiki' THEN (SELECT title FROM wiki_pages WHERE id = r.target_id) \
       WHEN 'project' THEN (SELECT name FROM projects WHERE id = r.target_id) \
       WHEN 'comment' THEN (SELECT substr(content, 1, 80) FROM comments WHERE id = r.target_id) \
     END AS target_title, \
     CASE r.target_kind \
       WHEN 'forum_post' THEN (SELECT COUNT(*) FROM forum_posts WHERE id = r.target_id AND deleted_at IS NOT NULL) \
       WHEN 'forum_comment' THEN (SELECT COUNT(*) FROM forum_comments WHERE id = r.target_id AND deleted_at IS NOT NULL) \
       WHEN 'wiki' THEN (SELECT COUNT(*) FROM wiki_pages WHERE id = r.target_id AND deleted_at IS NOT NULL) \
       WHEN 'project' THEN (SELECT COUNT(*) FROM projects WHERE id = r.target_id AND deleted_at IS NOT NULL) \
       WHEN 'comment' THEN (SELECT COUNT(*) FROM comments WHERE id = r.target_id AND deleted_at IS NOT NULL) \
     END AS target_deleted \
     FROM content_reports r \
     LEFT JOIN users ru ON ru.id = r.reporter_id \
     LEFT JOIN users hu ON hu.id = r.handled_by";

/// Does the reported thing exist and is it still visible?
async fn target_is_live(state: &AppState, kind: &str, id: i64) -> Result<bool, AppError> {
    let sql = match kind {
        "forum_post" => "SELECT COUNT(*) FROM forum_posts WHERE id = ?1 AND deleted_at IS NULL",
        "forum_comment" => {
            "SELECT COUNT(*) FROM forum_comments WHERE id = ?1 AND deleted_at IS NULL"
        }
        "wiki" => {
            "SELECT COUNT(*) FROM wiki_pages WHERE id = ?1 AND deleted_at IS NULL \
             AND status = 'published'"
        }
        "project" => {
            "SELECT COUNT(*) FROM projects WHERE id = ?1 AND deleted_at IS NULL \
             AND status = 'approved'"
        }
        "comment" => "SELECT COUNT(*) FROM comments WHERE id = ?1 AND deleted_at IS NULL",
        _ => return Ok(false),
    };
    let found: i64 = sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(found > 0)
}
