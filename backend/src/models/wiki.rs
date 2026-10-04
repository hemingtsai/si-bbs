use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WikiStatus {
    Draft,
    Published,
}

impl WikiStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct WikiPage {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub category: String,
    pub content: String,
    pub status: String,
    pub author_id: i64,
    pub deleted_at: Option<NaiveDateTime>,
    pub deleted_by: Option<i64>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl WikiPage {
    pub fn status_enum(&self) -> WikiStatus {
        WikiStatus::parse(&self.status).unwrap_or(WikiStatus::Draft)
    }
}

/// Wiki page as returned by the API. `content` stays raw Markdown so the
/// frontend can render it with `marked` instead of trusting server-side HTML.
#[derive(Debug, Clone, Serialize)]
pub struct WikiPageOut {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub category: String,
    pub content: String,
    pub status: String,
    pub author_id: i64,
    pub author_username: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub struct WikiPageJoined {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub category: String,
    pub content: String,
    pub status: String,
    pub author_id: i64,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub author_username: Option<String>,
}

impl From<WikiPageJoined> for WikiPageOut {
    fn from(p: WikiPageJoined) -> Self {
        Self {
            id: p.id,
            title: p.title,
            slug: p.slug,
            category: p.category,
            content: p.content,
            status: p.status,
            author_id: p.author_id,
            author_username: p.author_username,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WikiInput {
    pub title: String,
    pub category: String,
    pub content: String,
    /// `draft` (default) or `published`.
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WikiListQuery {
    pub category: Option<String>,
    pub q: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

/// Turn a title into a URL slug. Non-ASCII titles (for example Chinese) have
/// no transliteration, so they fall back to a timestamp-based slug.
pub fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    let mut prev_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if ch.is_ascii() && !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        format!("page-{}", chrono::Utc::now().timestamp_millis())
    } else {
        slug
    }
}
