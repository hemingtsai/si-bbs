-- Reports were the missing half of moderation: content could be deleted, but there
-- was no way for a reader to say "this needs looking at", and no queue for staff to
-- work through. `status` moves open -> resolved (acted on) or dismissed (no action).
CREATE TABLE content_reports (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    reporter_id INTEGER NOT NULL REFERENCES users(id),
    target_kind TEXT NOT NULL CHECK(target_kind IN
        ('forum_post', 'forum_comment', 'wiki', 'project', 'comment')),
    target_id   INTEGER NOT NULL,
    reason      TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open', 'resolved', 'dismissed')),
    handled_by  INTEGER REFERENCES users(id),
    handled_at  DATETIME,
    note        TEXT,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- One report per user per target: a second click should not create a second
    -- row for staff to triage.
    UNIQUE(reporter_id, target_kind, target_id)
);

CREATE INDEX idx_reports_open ON content_reports(status, created_at);
CREATE INDEX idx_reports_target ON content_reports(target_kind, target_id);
