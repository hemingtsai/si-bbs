use std::env;

/// The placeholder development builds fall back to. It is a public string, so a
/// release build must never boot with it: anyone could mint an admin token.
/// It is therefore also the value that is rejected outright.
pub const DEV_JWT_SECRET: &str = "dev-secret-change-me";

/// Shortest secret accepted in a release build. `openssl rand -hex 32` yields
/// 64 bytes, so any properly generated secret clears this comfortably.
const MIN_RELEASE_SECRET_LEN: usize = 32;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    pub github_token: String,
    pub github_api_base: String,
}

/// Decide which JWT secret the process is allowed to run with.
///
/// `is_release` is a parameter rather than a `cfg!` lookup inside the body so the
/// rule can be unit-tested without mutating the process environment.
///
/// Debug builds keep the convenience default; release builds refuse anything
/// unset, left at the development value, or too short to be worth signing with.
pub fn resolve_jwt_secret(raw: Option<&str>, is_release: bool) -> Result<String, String> {
    let raw = raw.map(str::trim).unwrap_or_default();

    if raw.is_empty() || raw == DEV_JWT_SECRET {
        return if is_release {
            Err(format!(
                "JWT_SECRET is unset or still the built-in development value \
                 (\"{DEV_JWT_SECRET}\"). Release builds refuse to start with it because \
                 it is public knowledge, so anyone could forge an admin token. \
                 Generate one with `openssl rand -hex 32` and pass it in the environment."
            ))
        } else {
            Ok(DEV_JWT_SECRET.to_string())
        };
    }

    if is_release && raw.len() < MIN_RELEASE_SECRET_LEN {
        return Err(format!(
            "JWT_SECRET is only {} bytes long; a release build requires at least \
             {MIN_RELEASE_SECRET_LEN} (`openssl rand -hex 32` produces 64 bytes).",
            raw.len()
        ));
    }

    Ok(raw.to_string())
}

impl Config {
    /// Build the configuration from the environment, exiting with a diagnostic
    /// when it is unusable. A process that cannot sign trustworthy tokens must
    /// not start at all — silently falling back to a public secret is how a
    /// deployment hands out admin sessions.
    pub fn from_env() -> Self {
        match Self::try_from_env() {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!("fatal: {err}");
                std::process::exit(1);
            }
        }
    }

    /// Fallible variant of [`Config::from_env`], for callers that would rather
    /// handle the error than exit.
    pub fn try_from_env() -> Result<Self, String> {
        let is_release = !cfg!(debug_assertions);
        let raw_secret = env::var("JWT_SECRET").ok();
        let jwt_secret = resolve_jwt_secret(raw_secret.as_deref(), is_release)?;

        Ok(Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://si-bbs.db?mode=rwc".into()),
            jwt_secret,
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = "9f2c4b6e8a0d1f3c5b7e9a0d1f3c5b7e9a0d1f3c5b7e9a0d1f3c5b7e9a0d1f3c";

    #[test]
    fn release_refuses_a_missing_secret() {
        let err = resolve_jwt_secret(None, true).unwrap_err();
        assert!(err.contains("JWT_SECRET"), "{err}");
    }

    #[test]
    fn release_refuses_the_development_value() {
        assert!(resolve_jwt_secret(Some(DEV_JWT_SECRET), true).is_err());
    }

    #[test]
    fn release_refuses_blank_secrets() {
        assert!(resolve_jwt_secret(Some(""), true).is_err());
        assert!(resolve_jwt_secret(Some("   "), true).is_err());
    }

    #[test]
    fn release_refuses_a_secret_shorter_than_the_minimum() {
        let short = "a".repeat(MIN_RELEASE_SECRET_LEN - 1);
        assert!(resolve_jwt_secret(Some(&short), true).is_err());
        let exact = "a".repeat(MIN_RELEASE_SECRET_LEN);
        assert_eq!(resolve_jwt_secret(Some(&exact), true).unwrap(), exact);
    }

    #[test]
    fn release_accepts_a_generated_secret_and_trims_it() {
        assert_eq!(
            resolve_jwt_secret(Some(GENERATED), true).unwrap(),
            GENERATED.to_string()
        );
        assert_eq!(
            resolve_jwt_secret(Some(&format!("  {GENERATED}\n")), true).unwrap(),
            GENERATED.to_string()
        );
    }

    #[test]
    fn debug_keeps_the_convenience_default() {
        assert_eq!(
            resolve_jwt_secret(None, false).unwrap(),
            DEV_JWT_SECRET.to_string()
        );
        // Short but explicit secrets stay usable for local work and E2E runs.
        assert_eq!(
            resolve_jwt_secret(Some("e2e-test-secret"), false).unwrap(),
            "e2e-test-secret".to_string()
        );
    }
}
