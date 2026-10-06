-- Wiki pages had no history: `update` overwrote the row, so two editors meant silent
-- last-write-wins and nothing was recoverable.
--
-- `wiki_pages.revision` is the authoritative counter (bumped on every save) and is
-- what the update endpoint compares against to detect a stale edit.
-- `wiki_revisions` keeps one snapshot per save, so revision N is the page *after*
-- the Nth save; a revert therefore appends a new snapshot instead of rewriting
-- history.
ALTER TABLE wiki_pages ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
UPDATE wiki_pages SET revision = 1;

CREATE TABLE wiki_revisions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id     INTEGER NOT NULL REFERENCES wiki_pages(id) ON DELETE CASCADE,
    revision_no INTEGER NOT NULL,
    title       TEXT NOT NULL,
    slug        TEXT NOT NULL,
    category    TEXT NOT NULL,
    content     TEXT NOT NULL,
    status      TEXT NOT NULL,
    author_id   INTEGER NOT NULL REFERENCES users(id),
    comment     TEXT,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(page_id, revision_no)
);

CREATE INDEX idx_wiki_revisions_page ON wiki_revisions(page_id, revision_no DESC);
