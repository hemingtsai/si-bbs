use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// The three fixed boards. Stored as short slugs; labels are a frontend concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Board {
    Models,
    Tools,
    Life,
}

pub const BOARDS: [Board; 3] = [Board::Models, Board::Tools, Board::Life];

impl Board {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Tools => "tools",
            Self::Life => "life",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "models" => Some(Self::Models),
            "tools" => Some(Self::Tools),
            "life" => Some(Self::Life),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct ForumPost {
    pub id: i64,
    pub board: String,
    pub title: String,
    pub content: String,
    pub author_id: i64,
    pub is_featured: i64,
    pub likes_count: i64,
    pub comments_count: i64,
    pub deleted_at: Option<NaiveDateTime>,
    pub deleted_by: Option<i64>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// The row plus the author's username, as the API returns it.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ForumPostOut {
    pub id: i64,
    pub board: String,
    pub title: String,
    pub content: String,
    pub author_id: i64,
    pub author_username: Option<String>,
    pub is_featured: i64,
    pub likes_count: i64,
    pub comments_count: i64,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ForumComment {
    pub id: i64,
    pub post_id: i64,
    pub author_id: i64,
    pub author_username: Option<String>,
    pub content: String,
    pub likes_count: i64,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ForumRule {
    pub board: String,
    pub title: String,
    pub content: String,
    pub updated_by: Option<i64>,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardInfo {
    pub slug: String,
    pub post_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostInput {
    pub board: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommentInput {
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleInput {
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeaturedInput {
    pub featured: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListQuery {
    pub board: Option<String>,
    pub q: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}
