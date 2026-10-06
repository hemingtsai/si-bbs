-- Password recovery. The token is stored as a SHA-256 hash: the lookup has to be by
-- hash (an Argon2 hash is salted, so a row could not be found from the token), and a
-- leaked database must not hand out working reset links.
CREATE TABLE password_resets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    token_hash TEXT NOT NULL UNIQUE,
    expires_at DATETIME NOT NULL,
    used_at    DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_password_resets_user ON password_resets(user_id, created_at DESC);
