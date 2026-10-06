-- Privileged actions left no trace: a role change, a ban, a purge or a review
-- decision could only be reconstructed from the effect itself. This is the record.
--
-- `actor_id` is nullable so the log survives if an account is ever removed;
-- `detail` holds a short human-readable description of the change.
CREATE TABLE audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_id    INTEGER REFERENCES users(id),
    action      TEXT NOT NULL,
    target_kind TEXT NOT NULL,
    target_id   INTEGER,
    detail      TEXT,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_audit_log_recent ON audit_log(created_at DESC, id DESC);
CREATE INDEX idx_audit_log_actor ON audit_log(actor_id, created_at DESC);
CREATE INDEX idx_audit_log_action ON audit_log(action, created_at DESC);
