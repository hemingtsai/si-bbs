//! Attachment upload, download and deletion.

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::middleware::auth::require_auth;
use crate::models::user::Role;
use crate::routes::AppState;
use crate::services::upload;

/// One uploaded file, as the API describes it.
fn describe(id: i64, filename: &str, content_type: &str, size_bytes: i64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "filename": filename,
        "content_type": content_type,
        "size_bytes": size_bytes,
        // Markdown snippet, ready to paste into a wiki page or a post.
        "markdown": format!("![{filename}](/api/attachments/{id})"),
        "url": format!("/api/attachments/{id}"),
    })
}

/// `POST /api/attachments` — `multipart/form-data` with a `file` field.
///
/// The declared content type must be on the allowlist **and** match the file's
/// leading bytes, and the stored name is generated here: the client's filename is
/// kept only as a label.
pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let claims = require_auth(&state, &headers).await?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("malformed upload: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let declared = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let filename = upload::safe_filename(field.file_name().unwrap_or("upload"));

        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("could not read upload: {e}")))?;

        if bytes.is_empty() {
            return Err(AppError::BadRequest("the uploaded file is empty".into()));
        }
        if bytes.len() > state.cfg.max_upload_bytes {
            return Err(AppError::BadRequest(format!(
                "the uploaded file exceeds {} bytes",
                state.cfg.max_upload_bytes
            )));
        }
        if !upload::declared_matches_body(&declared, &bytes) {
            return Err(AppError::BadRequest(
                "unsupported file type, or the content does not match the declared type".into(),
            ));
        }
        // Store under the sniffed type, never the client's claim.
        let content_type = upload::sniff(&bytes)
            .unwrap_or_else(|| declared.split(';').next().unwrap_or("text/plain").trim())
            .to_string();
        let extension = upload::extension_for(&content_type);

        let seed = crate::services::password_reset::generate_token();
        let dir = upload::storage_dir(&seed);
        let storage_path = format!("{dir}/{seed}.{extension}");
        let absolute = std::path::Path::new(&state.cfg.upload_dir).join(&storage_path);
        if let Some(parent) = absolute.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(format!("could not create upload dir: {e}")))?;
        }
        let size = bytes.len() as i64;
        tokio::fs::write(&absolute, &bytes)
            .await
            .map_err(|e| AppError::Internal(format!("could not write upload: {e}")))?;

        let res = sqlx::query(
            "INSERT INTO attachments \
             (uploader_id, filename, content_type, size_bytes, storage_path) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(claims.sub)
        .bind(&filename)
        .bind(&content_type)
        .bind(size)
        .bind(&storage_path)
        .execute(&state.pool)
        .await;

        let id = match res {
            Ok(done) => done.last_insert_rowid(),
            Err(e) => {
                // Do not leave an orphan file behind if the row cannot be written.
                let _ = tokio::fs::remove_file(&absolute).await;
                return Err(AppError::Internal(e.to_string()));
            }
        };

        return Ok((
            StatusCode::CREATED,
            Json(describe(id, &filename, &content_type, size)),
        ));
    }

    Err(AppError::BadRequest("no file field in the upload".into()))
}

/// `GET /api/attachments/{id}` — the bytes, with the stored type.
pub async fn serve(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT content_type, filename, storage_path FROM attachments \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (content_type, filename, storage_path) = row.ok_or(AppError::NotFound)?;

    let absolute = std::path::Path::new(&state.cfg.upload_dir).join(&storage_path);
    let bytes = match tokio::fs::read(&absolute).await {
        Ok(b) => b,
        Err(err) => {
            // A row without a file is an operator problem, not a client one.
            tracing::error!(attachment_id = id, error = %err, path = %absolute.display(), "attachment file missing");
            return Err(AppError::NotFound);
        }
    };

    // Images render inline; anything else downloads, so a PDF or a text file is not
    // handed to the browser as a document in our origin.
    let disposition = if content_type.starts_with("image/") {
        "inline".to_string()
    } else {
        format!("attachment; filename=\"{filename}\"")
    };

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
            // Uploads are immutable once stored, so they can be cached hard.
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        Body::from(bytes),
    )
        .into_response())
}

/// `DELETE /api/attachments/{id}` — uploader or staff. Soft delete, so the bin can
/// bring it back; the file stays on disk until the row is purged.
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let claims = require_auth(&state, &headers).await?;

    let uploader: i64 = sqlx::query_scalar(
        "SELECT uploader_id FROM attachments WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let staff = matches!(
        Role::parse(&claims.role),
        Some(Role::Admin) | Some(Role::Moderator)
    );
    if uploader != claims.sub && !staff {
        return Err(AppError::Forbidden);
    }

    sqlx::query(
        "UPDATE attachments SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ?2 \
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(claims.sub)
    .execute(&state.pool)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Remove the file backing a purged attachment. Best effort: the row is already
/// gone by the time this runs, and a leftover file is preferable to a failed purge.
pub async fn remove_stored_file(state: &AppState, storage_path: &str) {
    let absolute = std::path::Path::new(&state.cfg.upload_dir).join(storage_path);
    match tokio::fs::remove_file(&absolute).await {
        Ok(_) => tracing::info!(path = %absolute.display(), "purged attachment file"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(error = %err, path = %absolute.display(), "could not remove attachment file")
        }
    }
}
