use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::error::AppError;
use crate::middleware::auth::require_auth;
use crate::models::page::Page;
use crate::models::user::Role;
use crate::models::wiki::{
    WikiInput, WikiListQuery, WikiPageJoined, WikiPageOut, WikiStatus, slugify,
};
use crate::routes::AppState;
use crate::services::reports;

const MAX_TITLE_LEN: usize = 200;
const MAX_CATEGORY_LEN: usize = 40;
const MAX_CONTENT_LEN: usize = 200_000;

const PAGE_SELECT: &str = "SELECT w.id, w.title, w.slug, w.category, w.content, w.status, \
     w.author_id, w.created_at, w.updated_at, u.username AS author_username \
     FROM wiki_pages w LEFT JOIN users u ON u.id = w.author_id";

fn is_staff(role: &str) -> bool {
    matches!(Role::parse(role), Some(Role::Admin) | Some(Role::Moderator))
}

fn validate(input: &WikiInput) -> Result<(String, String, String, WikiStatus), AppError> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if title.chars().count() > MAX_TITLE_LEN {
        return Err(AppError::BadRequest("title is too long".into()));
    }

    let category = input.category.trim();
    if category.is_empty() {
        return Err(AppError::BadRequest("category is required".into()));
    }
    if category.chars().count() > MAX_CATEGORY_LEN {
        return Err(AppError::BadRequest("category is too long".into()));
    }

    let content = input.content.trim();
    if content.is_empty() {
        return Err(AppError::BadRequest("content is required".into()));
    }
    if content.chars().count() > MAX_CONTENT_LEN {
        return Err(AppError::BadRequest("content is too long".into()));
    }

    let status = match input.status.as_deref().map(str::trim) {
        None | Some("") | Some("draft") => WikiStatus::Draft,
        Some("published") => WikiStatus::Published,
        Some(_) => {
            return Err(AppError::BadRequest(
                "status must be draft or published".into(),
            ));
        }
    };

    Ok((
        title.to_string(),
        category.to_string(),
        content.to_string(),
        status,
    ))
}

/// Append a numeric suffix until the slug is free.
async fn unique_slug(state: &AppState, base: &str) -> String {
    let mut candidate = base.to_string();
    let mut n = 2;
    while sqlx::query_scalar::<_, i64>("SELECT id FROM wiki_pages WHERE slug = ?1")
        .bind(&candidate)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// Published pages, public. Supports category, keyword search and paging.
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<WikiListQuery>,
) -> Result<Json<Page<WikiPageOut>>, AppError> {
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let category = q
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let keyword =
        q.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"));

    let filters = "w.deleted_at IS NULL AND w.status = 'published' \
                   AND (?1 IS NULL OR w.category = ?1) \
                   AND (?2 IS NULL OR w.title LIKE ?2 OR w.content LIKE ?2)";

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM wiki_pages w WHERE {filters}"
    ))
    .bind(category)
    .bind(keyword.as_deref())
    .fetch_one(&state.pool)
    .await?;

    let sql = format!(
        "{PAGE_SELECT} WHERE {filters} ORDER BY w.updated_at DESC, w.id DESC LIMIT ?3 OFFSET ?4"
    );
    let rows: Vec<WikiPageJoined> = sqlx::query_as(&sql)
        .bind(category)
        .bind(keyword.as_deref())
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(Page {
        items: rows.into_iter().map(WikiPageOut::from).collect(),
        total,
        page,
        per_page,
    }))
}

/// Single page by slug. Drafts are visible to their author and to staff.
pub async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<WikiPageOut>, AppError> {
    let sql = format!("{PAGE_SELECT} WHERE w.slug = ?1 AND w.deleted_at IS NULL");
    let page: Option<WikiPageJoined> = sqlx::query_as(&sql)
        .bind(&slug)
        .fetch_optional(&state.pool)
        .await?;

    let page = match page {
        Some(p) => p,
        None => return Err(AppError::NotFound),
    };

    if page.status != WikiStatus::Published.as_str() {
        let claims = require_auth(&state, &headers).await.ok();
        let allowed = claims
            .as_ref()
            .is_some_and(|c| c.sub == page.author_id || is_staff(&c.role));
        if !allowed {
            return Err(AppError::NotFound);
        }
    }

    Ok(Json(page.into()))
}

/// Drafts and unpublished pages of the current user (plus all pages for staff).
pub async fn mine(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WikiListQuery>,
) -> Result<Json<Page<WikiPageOut>>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);

    let (where_sql, binds): (String, Vec<i64>) = if is_staff(&claims.role) {
        ("w.deleted_at IS NULL".to_string(), vec![])
    } else {
        (
            "w.deleted_at IS NULL AND w.author_id = ?1".to_string(),
            vec![claims.sub],
        )
    };

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM wiki_pages w WHERE {where_sql}"
    ))
    .bind(binds.first().copied())
    .fetch_one(&state.pool)
    .await?;

    let sql = format!(
        "{PAGE_SELECT} WHERE {where_sql} ORDER BY w.updated_at DESC, w.id DESC LIMIT ?2 OFFSET ?3"
    );
    let rows: Vec<WikiPageJoined> = sqlx::query_as(&sql)
        .bind(binds.first().copied())
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(Page {
        items: rows.into_iter().map(WikiPageOut::from).collect(),
        total,
        page,
        per_page,
    }))
}

/// Create a page. The slug is derived from the title and kept unique.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<WikiInput>,
) -> Result<(StatusCode, Json<WikiPageOut>), AppError> {
    let claims = require_auth(&state, &headers).await?;
    let (title, category, content, status) = validate(&input)?;

    let slug = unique_slug(&state, &slugify(&title)).await;

    let res = sqlx::query(
        "INSERT INTO wiki_pages (title, slug, category, content, status, author_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&title)
    .bind(&slug)
    .bind(&category)
    .bind(&content)
    .bind(status.as_str())
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;

    let id = res.last_insert_rowid();
    let page: WikiPageOut = fetch_page(&state, id).await?.into();
    Ok((StatusCode::CREATED, Json(page)))
}

/// Update a page. Author, moderator or admin only.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<WikiInput>,
) -> Result<Json<WikiPageOut>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let (title, category, content, status) = validate(&input)?;

    let existing = fetch_page(&state, id).await?;
    if existing.author_id != claims.sub && !is_staff(&claims.role) {
        return Err(AppError::Forbidden);
    }

    sqlx::query(
        "UPDATE wiki_pages SET title = ?2, category = ?3, content = ?4, status = ?5, \
         updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(&title)
    .bind(&category)
    .bind(&content)
    .bind(status.as_str())
    .execute(&state.pool)
    .await?;

    let page: WikiPageOut = fetch_page(&state, id).await?.into();
    Ok(Json(page))
}

/// Soft delete a page. Author, moderator or admin only.
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let existing = fetch_page(&state, id).await?;
    if existing.author_id != claims.sub && !is_staff(&claims.role) {
        return Err(AppError::Forbidden);
    }

    sqlx::query(
        "UPDATE wiki_pages SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ?2 \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;

    reports::resolve_for_target(&state.pool, "wiki", id, claims.sub, "content removed").await;
    Ok(StatusCode::NO_CONTENT)
}

/// Categories with their published page counts, for the sidebar.
pub async fn categories(
    State(state): State<AppState>,
) -> Result<Json<Vec<CategoryCount>>, AppError> {
    let rows: Vec<CategoryCount> = sqlx::query_as(
        "SELECT category, COUNT(*) AS count FROM wiki_pages \
         WHERE deleted_at IS NULL AND status = 'published' GROUP BY category \
         ORDER BY category ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct CategoryCount {
    pub category: String,
    pub count: i64,
}

async fn fetch_page(state: &AppState, id: i64) -> Result<WikiPageJoined, AppError> {
    let sql = format!("{PAGE_SELECT} WHERE w.id = ?1 AND w.deleted_at IS NULL");
    sqlx::query_as::<_, WikiPageJoined>(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)
}
