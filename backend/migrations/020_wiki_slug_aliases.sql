-- Old slugs of renamed pages. Without this, renaming a page broke every existing
-- link to it — which is why the slug was previously immutable.
--
-- ON DELETE CASCADE matters: purging a trashed page from the bin must not fail on
-- this reference (the same foreign-key trap the purge path already handles for
-- comments and replies).
CREATE TABLE wiki_slug_aliases (
    slug       TEXT PRIMARY KEY,
    page_id    INTEGER NOT NULL REFERENCES wiki_pages(id) ON DELETE CASCADE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_wiki_slug_aliases_page ON wiki_slug_aliases(page_id);
