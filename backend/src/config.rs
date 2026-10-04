use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    pub github_token: String,
    pub github_api_base: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://si-bbs.db?mode=rwc".into()),
            jwt_secret: env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-me".into()),
            access_ttl_secs: env::var("ACCESS_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            refresh_ttl_secs: env::var("REFRESH_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(7 * 24 * 3600),
            github_token: env::var("GITHUB_TOKEN").unwrap_or_default(),
            github_api_base: env::var("GITHUB_API_BASE")
                .unwrap_or_else(|_| "https://api.github.com".to_string()),
        }
    }
}
