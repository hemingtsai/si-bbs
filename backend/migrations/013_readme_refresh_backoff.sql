-- `readme_fetched_at` records the last *successful* fetch, which is what tells a
-- reader how old the cached document is. It therefore could not also be used to
-- throttle retries: while GitHub was down every single detail request for a stale
-- project made another upstream call, each waiting up to the 8s client timeout.
--
-- A separate column records the last *attempt*, successful or not, so failures
-- back off without pretending the data was refreshed.
ALTER TABLE projects ADD COLUMN readme_attempted_at DATETIME;
