-- Self-service profile. `display_name` is what readers see in place of the login
-- name; `avatar_url` is an external image (no uploads yet); `bio` is plain text.
ALTER TABLE users ADD COLUMN display_name TEXT;
ALTER TABLE users ADD COLUMN bio TEXT;
ALTER TABLE users ADD COLUMN avatar_url TEXT;
