-- Banned members keep their row (so content and audit trails stay intact) but
-- can no longer log in or use role-guarded endpoints.
ALTER TABLE users ADD COLUMN banned INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_users_banned ON users(banned) WHERE banned = 1;
