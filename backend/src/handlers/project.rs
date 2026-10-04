use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{Duration, Utc};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{require_auth, require_role};
use crate::models::page::Page;
use crate::models::project::{Project, ProjectOut, ProjectStatus};
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::github::{self, GithubClient};

/// A README older than this is refreshed on the next detail request.
const README_TTL_HOURS: i64 = 24;

const PROJECT_COLUMNS: &str = "id, name, github_url, owner, repo, description, readme_raw, \
     language, stars, forks, license, topics, category, status, submitted_by, reviewed_by, \
     review_note, readme_fetched_at, created_at, updated_at";

#[derive(Debug, Deserialize)]
pub struct SubmitInput {
    pub github_url: String,
    pub category: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub category: Option<String>,
    pub q: Option<String>,
    /// `stars` (default) or `recent`.
    pub sort: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewInput {
    /// `approve` or `reject`.
    pub action: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MineQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

fn paging(page: Option<i64>, per_page: Option<i64>) -> (i64, i64) {
    let per_page = per_page.unwrap_or(20).clamp(1, 50);
    let page = page.unwrap_or(1).max(1);
    (page, per_page)
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SubmitInput>,
) -> Result<(StatusCode, Json<ProjectOut>), AppError> {
    let claims = require_auth(&state.cfg, &headers)?;

    let repo_ref = github::parse_repo_url(&input.github_url)?;
    let category = input.category.trim();
    if category.is_empty() {
        return Err(AppError::BadRequest("category is required".into()));
    }
    if category.chars().count() > 40 {
        return Err(AppError::BadRequest("category is too long".into()));
    }

    let meta = GithubClient::new(&state.cfg)?.fetch_repo(&repo_ref).await?;

    let canonical_url = format!("https://github.com/{}/{}", meta.owner, meta.repo);
    let description = input
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .or_else(|| meta.description.clone());

    let res = sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, description, readme_raw, \
         language, stars, forks, license, topics, category, status, submitted_by, \
         readme_fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'pending', \
         ?13, CURRENT_TIMESTAMP)",
    )
    .bind(&meta.name)
    .bind(&canonical_url)
    .bind(&meta.owner)
    .bind(&meta.repo)
    .bind(description)
    .bind(meta.readme.as_deref())
    .bind(meta.language.as_deref())
    .bind(meta.stars)
    .bind(meta.forks)
    .bind(meta.license.as_deref())
    .bind(meta.topics.join(","))
    .bind(category)
    .bind(claims.sub)
    .execute(&state.pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict("this project has already been submitted".into())
        }
        other => AppError::from(other),
    })?;

    let id = res.last_insert_rowid();
    let project = fetch_project(&state, id).await?;
    Ok((StatusCode::CREATED, Json(project.into())))
}

/// Approved projects, public. Supports category, keyword, sorting and paging.
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Page<ProjectOut>>, AppError> {
    let (page, per_page) = paging(q.page, q.per_page);
    let keyword =
        q.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"));
    let category = q
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let order = match q.sort.as_deref() {
        Some("recent") => "created_at DESC, id DESC",
        Some("name") => "name COLLATE NOCASE ASC, id ASC",
        _ => "stars DESC, id DESC",
    };

    let filters = "deleted_at IS NULL AND status = 'approved' \
                   AND (?1 IS NULL OR category = ?1) \
                   AND (?2 IS NULL OR name LIKE ?2 OR IFNULL(description, '') LIKE ?2)";

    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM projects WHERE {filters}"))
        .bind(category)
        .bind(keyword.as_deref())
        .fetch_one(&state.pool)
        .await?;

    let sql = format!(
        "SELECT {PROJECT_COLUMNS} FROM projects WHERE {filters} ORDER BY {order} LIMIT ?3 OFFSET ?4"
    );
    let rows: Vec<Project> = sqlx::query_as(&sql)
        .bind(category)
        .bind(keyword.as_deref())
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(Page {
        items: rows.into_iter().map(ProjectOut::from).collect(),
        total,
        page,
        per_page,
    }))
}

/// Project detail. README is refreshed lazily when the cached copy is older
/// than 24h; a GitHub failure keeps the stale copy instead of erroring.
pub async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<ProjectOut>, AppError> {
    let mut project = fetch_project(&state, id).await?;
    let viewer = require_auth(&state.cfg, &headers).ok();

    let is_owner = viewer
        .as_ref()
        .is_some_and(|c| c.sub == project.submitted_by);
    let is_staff = viewer
        .as_ref()
        .and_then(|c| Role::parse(&c.role))
        .is_some_and(|r| matches!(r, Role::Admin | Role::Moderator));
    let approved = project.status_enum() == ProjectStatus::Approved;
    if !approved && !is_owner && !is_staff {
        return Err(AppError::NotFound);
    }

    if refresh_readme_if_stale(&state, &mut project).await {
        project = fetch_project(&state, id).await?;
    }

    Ok(Json(project.into()))
}

/// Submissions of the authenticated user, any status.
pub async fn mine(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MineQuery>,
) -> Result<Json<Page<ProjectOut>>, AppError> {
    let claims = require_auth(&state.cfg, &headers)?;
    let (page, per_page) = paging(q.page, q.per_page);

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE deleted_at IS NULL AND submitted_by = ?1",
    )
    .bind(claims.sub)
    .fetch_one(&state.pool)
    .await?;

    let sql = format!(
        "SELECT {PROJECT_COLUMNS} FROM projects WHERE deleted_at IS NULL AND submitted_by = ?1 \
         ORDER BY created_at DESC, id DESC LIMIT ?2 OFFSET ?3"
    );
    let rows: Vec<Project> = sqlx::query_as(&sql)
        .bind(claims.sub)
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(Page {
        items: rows.into_iter().map(ProjectOut::from).collect(),
        total,
        page,
        per_page,
    }))
}

/// Approve or reject a pending project. Moderator and admin only.
pub async fn review(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<ReviewInput>,
) -> Result<Json<ProjectOut>, AppError> {
    let claims = require_role(&state.cfg, &headers, &[Role::Admin, Role::Moderator])?;

    let status = match input.action.trim().to_ascii_lowercase().as_str() {
        "approve" | "approved" => ProjectStatus::Approved,
        "reject" | "rejected" => ProjectStatus::Rejected,
        _ => {
            return Err(AppError::BadRequest(
                "action must be approve or reject".into(),
            ));
        }
    };

    let note = input
        .note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if status == ProjectStatus::Rejected && note.is_none() {
        return Err(AppError::BadRequest(
            "a review note is required when rejecting".into(),
        ));
    }

    let res = sqlx::query(
        "UPDATE projects SET status = ?2, reviewed_by = ?3, review_note = ?4, \
         updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(claims.sub)
    .bind(note.as_deref())
    .execute(&state.pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    let project = fetch_project(&state, id).await?;
    Ok(Json(project.into()))
}

/// Queue for moderators: pending submissions, oldest first.
pub async fn review_queue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MineQuery>,
) -> Result<Json<Page<ProjectOut>>, AppError> {
    require_role(&state.cfg, &headers, &[Role::Admin, Role::Moderator])?;
    let (page, per_page) = paging(q.page, q.per_page);

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE deleted_at IS NULL AND status = 'pending'",
    )
    .fetch_one(&state.pool)
    .await?;

    let sql = format!(
        "SELECT {PROJECT_COLUMNS} FROM projects WHERE deleted_at IS NULL AND status = 'pending' \
         ORDER BY created_at ASC, id ASC LIMIT ?1 OFFSET ?2"
    );
    let rows: Vec<Project> = sqlx::query_as(&sql)
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(Page {
        items: rows.into_iter().map(ProjectOut::from).collect(),
        total,
        page,
        per_page,
    }))
}

pub async fn fetch_project(state: &AppState, id: i64) -> Result<Project, AppError> {
    let sql =
        format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1 AND deleted_at IS NULL");
    sqlx::query_as::<_, Project>(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)
}

/// Fetch a project that is publicly visible. Anything not approved 404s so the
/// catalogue and its comments never leak pending or rejected submissions.
pub async fn fetch_public_project(state: &AppState, id: i64) -> Result<Project, AppError> {
    let project = fetch_project(state, id).await?;
    if project.status_enum() != ProjectStatus::Approved {
        return Err(AppError::NotFound);
    }
    Ok(project)
}

/// Refresh `readme_raw` when the cache is older than 24h. Returns whether the
/// stored copy changed. GitHub errors are logged and the stale copy is kept.
async fn refresh_readme_if_stale(state: &AppState, project: &mut Project) -> bool {
    let stale = match project.readme_fetched_at {
        None => true,
        Some(ts) => Utc::now().naive_utc() - ts > Duration::hours(README_TTL_HOURS),
    };
    if !stale {
        return false;
    }

    let repo_ref = github::RepoRef {
        owner: project.owner.clone(),
        repo: project.repo.clone(),
    };
    let client = match GithubClient::new(&state.cfg) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(project_id = project.id, error = %e, "github client init failed");
            return false;
        }
    };

    match client.fetch_readme(&repo_ref).await {
        Ok(Some(readme)) => {
            let res = sqlx::query(
                "UPDATE projects SET readme_raw = ?2, readme_fetched_at = CURRENT_TIMESTAMP \
                 WHERE id = ?1",
            )
            .bind(project.id)
            .bind(&readme)
            .execute(&state.pool)
            .await;
            if let Err(e) = res {
                tracing::warn!(project_id = project.id, error = %e, "readme refresh write failed");
                return false;
            }
            project.readme_raw = Some(readme);
            project.readme_fetched_at = Some(Utc::now().naive_utc());
            true
        }
        Ok(None) => {
            // The repo has no README any more: drop the stale copy instead of
            // serving a document that no longer exists upstream.
            if let Err(e) = sqlx::query(
                "UPDATE projects SET readme_raw = NULL, readme_fetched_at = CURRENT_TIMESTAMP \
                 WHERE id = ?1",
            )
            .bind(project.id)
            .execute(&state.pool)
            .await
            {
                tracing::warn!(project_id = project.id, error = %e, "readme clear failed");
                return false;
            }
            project.readme_raw = None;
            project.readme_fetched_at = Some(Utc::now().naive_utc());
            true
        }
        Err(e) => {
            tracing::warn!(project_id = project.id, error = %e, "readme refresh failed, keeping cache");
            false
        }
    }
}
