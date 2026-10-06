use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::require_auth;
use crate::models::page::Page;
use crate::models::user::Role;
use crate::models::wiki::{
    RevertInput, WikiInput, WikiListQuery, WikiPageJoined, WikiPageOut, WikiRevision,
    WikiRevisionDetail, WikiStatus, slugify,
};
use crate::routes::AppState;
use crate::services::reports;

const MAX_TITLE_LEN: usize = 200;
const MAX_CATEGORY_LEN: usize = 40;
const MAX_CONTENT_LEN: usize = 200_000;

const PAGE_SELECT: &str = "SELECT w.id, w.title, w.slug, w.category, w.content, w.status, \
     w.author_id, w.revision, w.created_at, w.updated_at, \
     COALESCE(u.display_name, u.username) AS author_username \
     FROM wiki_pages w LEFT JOIN users u ON u.id = w.author_id";

/// Slugs that would shadow a static route, plus the shape a slug may take.
const RESERVED_SLUGS: [&str; 4] = ["mine", "categories", "page", "new"];
const MAX_SLUG_LEN: usize = 80;

/// Validate an explicit slug (used when a page's URL is being set by hand).
pub fn validate_slug(raw: &str) -> Result<String, AppError> {
    let slug = raw.trim().to_ascii_lowercase();
    if slug.is_empty() {
        return Err(AppError::BadRequest("slug is required".into()));
    }
    if slug.chars().count() > MAX_SLUG_LEN {
        return Err(AppError::BadRequest(format!(
            "slug cannot exceed {MAX_SLUG_LEN} characters"
        )));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::BadRequest(
            "slug may only contain lowercase letters, digits and hyphens".into(),
        ));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(AppError::BadRequest(
            "slug must not start or end with a hyphen".into(),
        ));
    }
    if RESERVED_SLUGS.contains(&slug.as_str()) {
        return Err(AppError::BadRequest(format!(
            "slug \"{slug}\" is reserved by the API"
        )));
    }
    Ok(slug)
}

/// Is the slug free (ignoring `except_id`)?
///
/// Aliases count as taken: if a *new* page could claim an old slug, the alias would
/// be shadowed and a link that used to reach the renamed page would silently point
/// at unrelated content.
async fn slug_available(state: &AppState, slug: &str, except_id: i64) -> bool {
    let taken: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM wiki_pages WHERE slug = ?1 AND id != ?2 \
         UNION ALL SELECT page_id FROM wiki_slug_aliases WHERE slug = ?1 AND page_id != ?2 \
         LIMIT 1",
    )
    .bind(slug)
    .bind(except_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    taken.is_none()
}

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
        None => {
            // Renamed page: follow the alias to its current slug. The response
            // carries the canonical slug, so the client can rewrite its URL.
            match sqlx::query_as::<_, WikiPageJoined>(&format!(
                "{PAGE_SELECT} WHERE w.id = (SELECT page_id FROM wiki_slug_aliases WHERE slug = ?1) \
                 AND w.deleted_at IS NULL"
            ))
            .bind(&slug)
            .fetch_optional(&state.pool)
            .await?
            {
                Some(p) => p,
                None => return Err(AppError::NotFound),
            }
        }
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

    // An explicit slug wins; otherwise derive one from the title.
    let slug = match input.slug.as_deref() {
        Some(raw) => {
            let wanted = validate_slug(raw)?;
            if !slug_available(&state, &wanted, 0).await {
                return Err(AppError::Conflict("that slug is already taken".into()));
            }
            wanted
        }
        None => unique_slug(&state, &slugify(&title)).await,
    };

    // Insert the page and its first revision together: a page without revision 1
    // would make the history endpoint lie about where the page started.
    let mut tx = state.pool.begin().await?;
    let res = sqlx::query(
        "INSERT INTO wiki_pages (title, slug, category, content, status, author_id, revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
    )
    .bind(&title)
    .bind(&slug)
    .bind(&category)
    .bind(&content)
    .bind(status.as_str())
    .bind(claims.sub)
    .execute(&mut *tx)
    .await?;

    let id = res.last_insert_rowid();
    sqlx::query(
        "INSERT INTO wiki_revisions \
         (page_id, revision_no, title, slug, category, content, status, author_id, comment) \
         VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(id)
    .bind(&title)
    .bind(&slug)
    .bind(&category)
    .bind(&content)
    .bind(status.as_str())
    .bind(claims.sub)
    .bind(
        input
            .comment
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty()),
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let page: WikiPageOut = fetch_page(&state, id).await?.into();
    Ok((StatusCode::CREATED, Json(page)))
}

/// Update a page. Author, moderator or admin only.
///
/// Concurrency: the write is a compare-and-set on `revision`. The editor's
/// `base_revision` (or, when omitted, the revision that was just read) must still
/// match, otherwise the save is refused with 409 and the caller can reload — no
/// more silent last-write-wins.
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
    let base = input.base_revision.unwrap_or(existing.revision);
    if base != existing.revision {
        return Err(AppError::Conflict(format!(
            "the page moved on: you edited revision {base}, the current one is {}",
            existing.revision
        )));
    }

    // Slug change: keep the old one as an alias so existing links keep working.
    let new_slug = match input.slug.as_deref() {
        None => existing.slug.clone(),
        Some(raw) => {
            let wanted = validate_slug(raw)?;
            if wanted != existing.slug && !slug_available(&state, &wanted, id).await {
                return Err(AppError::Conflict("that slug is already taken".into()));
            }
            wanted
        }
    };
    let slug_changed = new_slug != existing.slug;
    let next_revision = base + 1;

    let mut tx = state.pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE wiki_pages SET title = ?3, category = ?4, content = ?5, status = ?6, \
         slug = ?7, revision = ?8, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ?1 AND revision = ?2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(base)
    .bind(&title)
    .bind(&category)
    .bind(&content)
    .bind(status.as_str())
    .bind(&new_slug)
    .bind(next_revision)
    .execute(&mut *tx)
    .await?;
    // Somebody saved between the read above and this statement.
    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "the page was modified by someone else; reload and try again".into(),
        ));
    }

    sqlx::query(
        "INSERT INTO wiki_slug_aliases (slug, page_id) VALUES (?1, ?2) \
         ON CONFLICT(slug) DO UPDATE SET page_id = excluded.page_id",
    )
    .bind(&existing.slug)
    .bind(id)
    .execute(&mut *tx)
    .await
    .ok();

    sqlx::query(
        "INSERT INTO wiki_revisions \
         (page_id, revision_no, title, slug, category, content, status, author_id, comment) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(id)
    .bind(next_revision)
    .bind(&title)
    .bind(&new_slug)
    .bind(&category)
    .bind(&content)
    .bind(status.as_str())
    .bind(claims.sub)
    .bind(
        input
            .comment
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty()),
    )
    .execute(&mut *tx)
    .await?;
    // The page no longer answers on its own old slug.
    if slug_changed {
        sqlx::query("DELETE FROM wiki_slug_aliases WHERE slug = ?1")
            .bind(&new_slug)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

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

/// History of a page, newest first. Only metadata: a 200KB body per revision would
/// make the list useless.
///
/// Visibility follows the page: a published page's history is public (that is how
/// wikis work), a draft's is limited to its author and staff.
pub async fn revisions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<WikiListQuery>,
) -> Result<Json<Page<WikiRevision>>, AppError> {
    let page = fetch_page(&state, id).await?;
    if page.status != WikiStatus::Published.as_str() {
        let claims = require_auth(&state, &headers).await.ok();
        let allowed = claims
            .as_ref()
            .is_some_and(|c| c.sub == page.author_id || is_staff(&c.role));
        if !allowed {
            return Err(AppError::NotFound);
        }
    }

    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let page_no = q.page.unwrap_or(1).max(1);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wiki_revisions WHERE page_id = ?1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;

    let items: Vec<WikiRevision> = sqlx::query_as(
        "SELECT r.revision_no, r.title, r.slug, r.category, r.status, r.author_id, \
         COALESCE(u.display_name, u.username) AS author_username, r.comment, r.created_at, \
         length(r.content) AS content_chars \
         FROM wiki_revisions r LEFT JOIN users u ON u.id = r.author_id \
         WHERE r.page_id = ?1 ORDER BY r.revision_no DESC LIMIT ?2 OFFSET ?3",
    )
    .bind(id)
    .bind(per_page)
    .bind((page_no - 1) * per_page)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(Page {
        items,
        total,
        page: page_no,
        per_page,
    }))
}

/// One revision including its body.
pub async fn revision_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, no)): Path<(i64, i64)>,
) -> Result<Json<WikiRevisionDetail>, AppError> {
    let page = fetch_page(&state, id).await?;
    if page.status != WikiStatus::Published.as_str() {
        let claims = require_auth(&state, &headers).await.ok();
        let allowed = claims
            .as_ref()
            .is_some_and(|c| c.sub == page.author_id || is_staff(&c.role));
        if !allowed {
            return Err(AppError::NotFound);
        }
    }

    let revision: WikiRevisionDetail = sqlx::query_as(
        "SELECT r.revision_no, r.title, r.slug, r.category, r.status, r.author_id, \
         COALESCE(u.display_name, u.username) AS author_username, r.comment, r.content, \
         r.created_at \
         FROM wiki_revisions r LEFT JOIN users u ON u.id = r.author_id \
         WHERE r.page_id = ?1 AND r.revision_no = ?2",
    )
    .bind(id)
    .bind(no)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(revision))
}

/// Restore an older revision by appending it as a new one.
///
/// History is never rewritten: the revert is itself a revision, so it can be
/// reverted in turn and the audit trail stays linear.
pub async fn revert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, no)): Path<(i64, i64)>,
    Json(input): Json<RevertInput>,
) -> Result<Json<WikiPageOut>, AppError> {
    let claims = require_auth(&state, &headers).await?;
    let current = fetch_page(&state, id).await?;
    if current.author_id != claims.sub && !is_staff(&claims.role) {
        return Err(AppError::Forbidden);
    }

    let base = input.base_revision.unwrap_or(current.revision);
    if base != current.revision {
        return Err(AppError::Conflict(format!(
            "the page moved on: you reverted from revision {base}, the current one is {}",
            current.revision
        )));
    }

    let old: WikiRevisionDetail = sqlx::query_as(
        "SELECT r.revision_no, r.title, r.slug, r.category, r.status, r.author_id, \
         COALESCE(u.display_name, u.username) AS author_username, r.comment, r.content, \
         r.created_at \
         FROM wiki_revisions r LEFT JOIN users u ON u.id = r.author_id \
         WHERE r.page_id = ?1 AND r.revision_no = ?2",
    )
    .bind(id)
    .bind(no)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // The slug is not part of a revert: URLs are identity, not content, and the
    // alias table is what keeps old ones alive.
    let next_revision = base + 1;
    let comment = input
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("revert to revision {no}"));

    let mut tx = state.pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE wiki_pages SET title = ?3, category = ?4, content = ?5, status = ?6, \
         revision = ?7, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ?1 AND revision = ?2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(base)
    .bind(&old.title)
    .bind(&old.category)
    .bind(&old.content)
    .bind(&old.status)
    .bind(next_revision)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "the page was modified by someone else; reload and try again".into(),
        ));
    }

    sqlx::query(
        "INSERT INTO wiki_revisions \
         (page_id, revision_no, title, slug, category, content, status, author_id, comment) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(id)
    .bind(next_revision)
    .bind(&old.title)
    .bind(&current.slug)
    .bind(&old.category)
    .bind(&old.content)
    .bind(&old.status)
    .bind(claims.sub)
    .bind(&comment)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let page: WikiPageOut = fetch_page(&state, id).await?.into();
    Ok(Json(page))
}

#[derive(Debug, Deserialize, Default)]
pub struct DiffQuery {
    /// Revision to compare from. Required in practice; see the handler.
    pub from: Option<i64>,
    /// Revision to compare to; defaults to the page's current revision.
    pub to: Option<i64>,
}

/// Diff two revisions of a page. `?from=2` alone means "what changed in the latest
/// save".
pub async fn diff_revisions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<DiffQuery>,
) -> Result<Json<crate::services::diff::Diff>, AppError> {
    let page = fetch_page(&state, id).await?;
    if page.status != WikiStatus::Published.as_str() {
        let claims = require_auth(&state, &headers).await.ok();
        let allowed = claims
            .as_ref()
            .is_some_and(|c| c.sub == page.author_id || is_staff(&c.role));
        if !allowed {
            return Err(AppError::NotFound);
        }
    }

    let to = q.to.unwrap_or(page.revision);
    let from = q.from.unwrap_or(to - 1);
    if from < 1 || to < 1 {
        return Err(AppError::BadRequest(
            "from and to must be revision numbers of at least 1".into(),
        ));
    }
    if from == to {
        return Err(AppError::BadRequest(
            "from and to are the same revision".into(),
        ));
    }

    let bodies: Vec<(i64, String)> = sqlx::query_as(
        "SELECT revision_no, content FROM wiki_revisions \
         WHERE page_id = ?1 AND revision_no IN (?2, ?3)",
    )
    .bind(id)
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await?;
    let body = |rev: i64| {
        bodies
            .iter()
            .find(|(no, _)| *no == rev)
            .map(|(_, content)| content.clone())
    };
    let from_body = body(from).ok_or(AppError::NotFound)?;
    let to_body = body(to).ok_or(AppError::NotFound)?;

    crate::services::diff::diff(&from_body, &to_body, from, to)
        .map(Json)
        .map_err(AppError::BadRequest)
}
