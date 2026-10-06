-- Attachments (images, PDFs, plain text) stored on disk under UPLOAD_DIR and served
-- back by id. The row records the *sniffed* content type, not what the client
-- claimed, and `storage_path` is generated server-side.
CREATE TABLE attachments (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    uploader_id  INTEGER NOT NULL REFERENCES users(id),
    filename     TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    storage_path TEXT NOT NULL,
    deleted_at   DATETIME,
    deleted_by   INTEGER REFERENCES users(id),
    created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_attachments_uploader ON attachments(uploader_id, deleted_at);

-- Attachments join the bin like everything else, so a deleted upload can be
-- restored or purged through the same endpoints.
DROP VIEW trash_view;
CREATE VIEW trash_view AS
SELECT 'wiki'         AS kind, id, title AS name, deleted_at, deleted_by FROM wiki_pages      WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'project'      AS kind, id, name  AS name, deleted_at, deleted_by FROM projects        WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'comment'      AS kind, id, substr(content, 1, 50) AS name, deleted_at, deleted_by FROM comments WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'forum_post'   AS kind, id, title AS name, deleted_at, deleted_by FROM forum_posts     WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'forum_comment' AS kind, id, substr(content, 1, 50) AS name, deleted_at, deleted_by FROM forum_comments WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'attachment'   AS kind, id, filename AS name, deleted_at, deleted_by FROM attachments   WHERE deleted_at IS NOT NULL;
