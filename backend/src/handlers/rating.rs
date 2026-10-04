use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;

use crate::error::AppError;
use crate::handlers::project::fetch_public_project;
use crate::middleware::auth::require_auth;
use crate::models::rating::{RateAck, RateInput, RatingSummary};
use crate::routes::AppState;

const MIN_SCORE: i64 = 1;
const MAX_SCORE: i64 = 10;
const MAX_COMMENT_LEN: usize = 1000;

/// Create or update the caller's rating for a project (one rating per user).
pub async fn rate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<i64>,
    Json(input): Json<RateInput>,
) -> Result<Json<RateAck>, AppError> {
    let claims = require_auth(&state.cfg, &headers)?;

    if !(MIN_SCORE..=MAX_SCORE).contains(&input.score) {
        return Err(AppError::BadRequest(format!(
            "score must be between {MIN_SCORE} and {MAX_SCORE}"
        )));
    }
    let comment = input
        .comment
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    if comment
        .as_ref()
        .is_some_and(|c| c.chars().count() > MAX_COMMENT_LEN)
    {
        return Err(AppError::BadRequest("rating comment is too long".into()));
    }

    // Pending or rejected projects are not visible, so they cannot be rated.
    fetch_public_project(&state, project_id).await?;

    sqlx::query(
        "INSERT INTO ratings (project_id, user_id, score, comment) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(project_id, user_id) DO UPDATE SET score = excluded.score, \
         comment = excluded.comment, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(project_id)
    .bind(claims.sub)
    .bind(input.score)
    .bind(comment.as_deref())
    .execute(&state.pool)
    .await?;

    let summary = summary_for(&state, project_id).await?;
    Ok(Json(RateAck {
        summary,
        my_score: input.score,
    }))
}

/// Public aggregate: average score rounded to one decimal plus vote count.
pub async fn summary(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Json<RatingSummary>, AppError> {
    // Ensures a 404 for unknown or not-yet-approved projects.
    fetch_public_project(&state, project_id).await?;
    Ok(Json(summary_for(&state, project_id).await?))
}

async fn summary_for(state: &AppState, project_id: i64) -> Result<RatingSummary, AppError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        average: Option<f64>,
        count: i64,
    }

    let row = sqlx::query_as::<_, Row>(
        "SELECT AVG(score) AS average, COUNT(*) AS count FROM ratings WHERE project_id = ?1",
    )
    .bind(project_id)
    .fetch_one(&state.pool)
    .await?;

    let average = row
        .average
        .map(|a| (a * 10.0).round() / 10.0)
        .unwrap_or(0.0);
    Ok(RatingSummary {
        project_id,
        average,
        count: row.count,
    })
}
