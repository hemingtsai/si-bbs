use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use std::time::Duration;

use crate::config::Config;
use crate::error::AppError;

const USER_AGENT: &str = concat!("si-bbs/", env!("CARGO_PKG_VERSION"));

/// Owner/repo pair extracted from a GitHub URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub repo: String,
}

/// Metadata for a GitHub repository, normalised for the projects table.
#[derive(Debug, Clone)]
pub struct RepoMeta {
    pub name: String,
    pub owner: String,
    pub repo: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i64,
    pub forks: i64,
    pub license: Option<String>,
    pub topics: Vec<String>,
    pub readme: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GhLicense {
    spdx_id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRepo {
    name: String,
    owner: GhOwner,
    description: Option<String>,
    stargazers_count: i64,
    forks_count: i64,
    language: Option<String>,
    topics: Option<Vec<String>>,
    license: Option<GhLicense>,
}

#[derive(Debug, Deserialize)]
struct GhContent {
    content: String,
    encoding: String,
}

/// Accepts the common GitHub URL spellings and returns owner/repo.
///
/// Supported: `https://github.com/o/r`, `http://`, `www.`, trailing `/`,
/// `.git` suffix and the `git@github.com:o/r.git` SSH form.
pub fn parse_repo_url(url: &str) -> Result<RepoRef, AppError> {
    let raw = url.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("github url is required".into()));
    }

    let path = if let Some(rest) = raw.strip_prefix("git@github.com:") {
        rest
    } else {
        let rest = raw
            .strip_prefix("https://")
            .or_else(|| raw.strip_prefix("http://"))
            .ok_or_else(|| AppError::BadRequest("github url must be a github.com url".into()))?;
        let (host, path) = rest
            .split_once('/')
            .ok_or_else(|| AppError::BadRequest("github url must be a github.com url".into()))?;
        let host = host.to_ascii_lowercase();
        if host != "github.com" && host != "www.github.com" {
            return Err(AppError::BadRequest(
                "github url must be a github.com url".into(),
            ));
        }
        path
    };

    let mut segments = path
        .split(['/', ':', '?'])
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let owner = segments.next().unwrap_or_default().to_string();
    let repo_raw = segments.next().unwrap_or_default();
    let repo = repo_raw
        .strip_suffix(".git")
        .unwrap_or(repo_raw)
        .to_string();

    if segments.next().is_some() || owner.is_empty() || repo.is_empty() {
        return Err(AppError::BadRequest(
            "github url must look like https://github.com/owner/repo".into(),
        ));
    }

    let valid = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    if !valid(&owner) || !valid(&repo) {
        return Err(AppError::BadRequest(
            "invalid owner or repository name".into(),
        ));
    }

    Ok(RepoRef { owner, repo })
}

/// Thin GitHub REST client. `GITHUB_API_BASE` is overridable so integration
/// tests can point it at a mock server.
#[derive(Clone)]
pub struct GithubClient {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl GithubClient {
    pub fn new(cfg: &Config) -> Result<Self, AppError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| AppError::Internal(format!("http client: {e}")))?;
        Ok(Self {
            http,
            base: cfg.github_api_base.trim_end_matches('/').to_string(),
            token: cfg.github_token.clone(),
        })
    }

    fn request(&self, url: String) -> reqwest::RequestBuilder {
        let req = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if self.token.is_empty() {
            req
        } else {
            req.header("Authorization", format!("Bearer {}", self.token))
        }
    }

    /// Fetch repository metadata. A 404 from GitHub is surfaced as 400 so the
    /// submitter learns the repository does not exist.
    pub async fn fetch_repo(&self, r: &RepoRef) -> Result<RepoMeta, AppError> {
        let url = format!("{}/repos/{}/{}", self.base, r.owner, r.repo);
        let resp = self
            .request(url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("github request failed: {e}")))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::BadRequest("github repository not found".into()));
        }
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!(
                "github api returned {}",
                resp.status().as_u16()
            )));
        }

        let gh: GhRepo = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("github response invalid: {e}")))?;

        let license = gh
            .license
            .and_then(|l| l.spdx_id.filter(|s| s != "NOASSERTION").or(l.name));

        Ok(RepoMeta {
            name: gh.name.clone(),
            owner: gh.owner.login,
            repo: gh.name,
            description: gh.description.filter(|d| !d.trim().is_empty()),
            language: gh.language,
            stars: gh.stargazers_count,
            forks: gh.forks_count,
            license,
            topics: gh.topics.unwrap_or_default(),
            readme: self.fetch_readme(r).await.unwrap_or(None),
        })
    }

    /// Fetch the README and return it as raw Markdown. `None` means the repo
    /// simply has no README, which is not an error.
    pub async fn fetch_readme(&self, r: &RepoRef) -> Result<Option<String>, AppError> {
        let url = format!("{}/repos/{}/{}/readme", self.base, r.owner, r.repo);
        let resp = self
            .request(url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("github request failed: {e}")))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!(
                "github api returned {}",
                resp.status().as_u16()
            )));
        }

        let content: GhContent = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("github response invalid: {e}")))?;

        if content.encoding != "base64" {
            return Err(AppError::Internal(format!(
                "unexpected readme encoding {}",
                content.encoding
            )));
        }
        let cleaned: String = content
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = BASE64
            .decode(cleaned)
            .map_err(|e| AppError::Internal(format!("readme base64 invalid: {e}")))?;
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }
}

/// Pure helper used by tests and the lazy refresh path.
pub fn decode_readme_base64(content: &str) -> Option<String> {
    let cleaned: String = content.chars().filter(|c| !c.is_whitespace()).collect();
    BASE64
        .decode(cleaned)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}
