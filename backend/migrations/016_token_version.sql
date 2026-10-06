-- A password change has to be able to end other sessions. Tokens are stateless, so
-- the epoch lives on the user row and is copied into every token as `tv`; a token
-- whose `tv` no longer matches is refused. `require_auth` already reads this row on
-- every request, so the check is free.
ALTER TABLE users ADD COLUMN token_version INTEGER NOT NULL DEFAULT 0;
