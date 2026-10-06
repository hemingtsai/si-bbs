use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Pending,
    Approved,
    Rejected,
}

impl ProjectStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub github_url: String,
    pub owner: String,
    pub repo: String,
    pub description: Option<String>,
    pub readme_raw: Option<String>,
    pub language: Option<String>,
    pub stars: i64,
    pub forks: i64,
    pub license: Option<String>,
    pub topics: Option<String>,
    pub category: String,
    pub status: String,
    pub submitted_by: i64,
    pub reviewed_by: Option<i64>,
    pub review_note: Option<String>,
    pub readme_fetched_at: Option<NaiveDateTime>,
    /// Last refresh attempt, successful or not. Drives the failure backoff.
    pub readme_attempted_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl Project {
    pub fn status_enum(&self) -> ProjectStatus {
        ProjectStatus::parse(&self.status).unwrap_or(ProjectStatus::Pending)
    }

    pub fn topic_list(&self) -> Vec<String> {
        self.topics
            .as_deref()
            .map(|t| {
                t.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// API representation: topics are exposed as a list, README stays raw Markdown
/// so the frontend can render it with `marked`.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectOut {
    pub id: i64,
    pub name: String,
    pub github_url: String,
    pub owner: String,
    pub repo: String,
    pub description: Option<String>,
    pub readme_raw: Option<String>,
    pub language: Option<String>,
    pub stars: i64,
    pub forks: i64,
    pub license: Option<String>,
    pub topics: Vec<String>,
    pub category: String,
    pub status: String,
    pub submitted_by: i64,
    pub reviewed_by: Option<i64>,
    pub review_note: Option<String>,
    pub readme_fetched_at: Option<NaiveDateTime>,
    pub readme_attempted_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<Project> for ProjectOut {
    fn from(p: Project) -> Self {
        let topics = p.topic_list();
        Self {
            id: p.id,
            name: p.name,
            github_url: p.github_url,
            owner: p.owner,
            repo: p.repo,
            description: p.description,
            readme_raw: p.readme_raw,
            language: p.language,
            stars: p.stars,
            forks: p.forks,
            license: p.license,
            topics,
            category: p.category,
            status: p.status,
            submitted_by: p.submitted_by,
            reviewed_by: p.reviewed_by,
            review_note: p.review_note,
            readme_fetched_at: p.readme_fetched_at,
            readme_attempted_at: p.readme_attempted_at,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}
