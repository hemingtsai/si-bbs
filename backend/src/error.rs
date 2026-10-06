use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    NotFound,
    BadRequest(String),
    Conflict(String),
    Unauthorized,
    Forbidden,
    /// Carries the wait so the response can advertise `Retry-After`.
    TooManyRequests {
        retry_after_secs: u64,
    },
    /// The proof-of-work challenge was missing, unsolved, expired or replayed.
    ///
    /// Rendered with `code: "pow"` so the frontend can tell "go and solve a new
    /// challenge, then retry" apart from an ordinary validation failure.
    Challenge(String),
    Internal(String),
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Challenge(_) => StatusCode::BAD_REQUEST,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::BadRequest(m) | Self::Conflict(m) | Self::Challenge(m) => write!(f, "{m}"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::Forbidden => write!(f, "forbidden"),
            Self::TooManyRequests { retry_after_secs } => {
                write!(f, "too many requests, retry in {retry_after_secs}s")
            }
            Self::Internal(m) => write!(f, "{m}"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status();
        let retry_after_secs = match &self {
            Self::TooManyRequests { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        };
        let body = match &self {
            // The client only ever gets a generic message, so this log line is the
            // only place the real cause survives: `From<sqlx::Error>` funnels the
            // driver message in here and nowhere else.
            Self::Internal(m) => {
                tracing::error!(error = %m, "internal server error");
                json!({ "error": "internal server error" })
            }
            Self::BadRequest(m) => json!({ "error": m }),
            Self::Conflict(m) => json!({ "error": m }),
            Self::NotFound => json!({ "error": "not found" }),
            Self::Unauthorized => json!({ "error": "unauthorized" }),
            Self::Forbidden => json!({ "error": "forbidden" }),
            Self::TooManyRequests { retry_after_secs } => json!({
                "error": format!("too many requests, retry in {retry_after_secs} seconds")
            }),
            // The `code` is the contract with the frontend: it means "solve a fresh
            // challenge and send this request again", not "your input was wrong".
            Self::Challenge(m) => json!({ "error": m, "code": "pow" }),
        };

        let mut response = (status, Json(body)).into_response();
        if let Some(secs) = retry_after_secs
            && let Ok(value) = axum::http::HeaderValue::from_str(&secs.to_string())
        {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
        response
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                Self::Conflict("already exists".into())
            }
            other => Self::Internal(other.to_string()),
        }
    }
}
