-- Keyword search was `LIKE '%kw%'`: no index, and `%`/`_` were not escaped, so a
-- search for "%" matched everything. FTS5 with the *trigram* tokenizer does
-- substring matching (which is what makes Chinese searchable — a Chinese phrase has
-- no spaces for a word tokenizer to split on) and gives a rank to sort by.
--
-- These are external-content tables: the index is derived from the content table, so
-- nothing is stored twice. `deleted_at` is filtered at query time rather than by
-- removing rows from the index, which keeps the triggers simple.
CREATE VIRTUAL TABLE wiki_fts USING fts5(
    title, content, content='wiki_pages', content_rowid='id', tokenize='trigram'
);
CREATE VIRTUAL TABLE forum_fts USING fts5(
    title, content, content='forum_posts', content_rowid='id', tokenize='trigram'
);
CREATE VIRTUAL TABLE projects_fts USING fts5(
    name, description, content='projects', content_rowid='id', tokenize='trigram'
);

-- Keep the index in step with the tables (the standard external-content triggers).
CREATE TRIGGER wiki_pages_fts_ai AFTER INSERT ON wiki_pages BEGIN
    INSERT INTO wiki_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
END;
CREATE TRIGGER wiki_pages_fts_ad AFTER DELETE ON wiki_pages BEGIN
    INSERT INTO wiki_fts(wiki_fts, rowid, title, content)
        VALUES ('delete', old.id, old.title, old.content);
END;
CREATE TRIGGER wiki_pages_fts_au AFTER UPDATE ON wiki_pages BEGIN
    INSERT INTO wiki_fts(wiki_fts, rowid, title, content)
        VALUES ('delete', old.id, old.title, old.content);
    INSERT INTO wiki_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
END;

CREATE TRIGGER forum_posts_fts_ai AFTER INSERT ON forum_posts BEGIN
    INSERT INTO forum_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
END;
CREATE TRIGGER forum_posts_fts_ad AFTER DELETE ON forum_posts BEGIN
    INSERT INTO forum_fts(forum_fts, rowid, title, content)
        VALUES ('delete', old.id, old.title, old.content);
END;
CREATE TRIGGER forum_posts_fts_au AFTER UPDATE ON forum_posts BEGIN
    INSERT INTO forum_fts(forum_fts, rowid, title, content)
        VALUES ('delete', old.id, old.title, old.content);
    INSERT INTO forum_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
END;

CREATE TRIGGER projects_fts_ai AFTER INSERT ON projects BEGIN
    INSERT INTO projects_fts(rowid, name, description) VALUES (new.id, new.name, new.description);
END;
CREATE TRIGGER projects_fts_ad AFTER DELETE ON projects BEGIN
    INSERT INTO projects_fts(projects_fts, rowid, name, description)
        VALUES ('delete', old.id, old.name, old.description);
END;
CREATE TRIGGER projects_fts_au AFTER UPDATE ON projects BEGIN
    INSERT INTO projects_fts(projects_fts, rowid, name, description)
        VALUES ('delete', old.id, old.name, old.description);
    INSERT INTO projects_fts(rowid, name, description) VALUES (new.id, new.name, new.description);
END;

-- Index whatever already exists.
INSERT INTO wiki_fts(rowid, title, content) SELECT id, title, content FROM wiki_pages;
INSERT INTO forum_fts(rowid, title, content) SELECT id, title, content FROM forum_posts;
INSERT INTO projects_fts(rowid, name, description) SELECT id, name, description FROM projects;
