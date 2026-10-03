ALTER TABLE wiki_pages ADD COLUMN deleted_at DATETIME;
ALTER TABLE wiki_pages ADD COLUMN deleted_by INTEGER REFERENCES users(id);
CREATE INDEX idx_wiki_deleted_at ON wiki_pages(deleted_at) WHERE deleted_at IS NOT NULL;

ALTER TABLE projects ADD COLUMN deleted_at DATETIME;
ALTER TABLE projects ADD COLUMN deleted_by INTEGER REFERENCES users(id);
CREATE INDEX idx_projects_deleted_at ON projects(deleted_at) WHERE deleted_at IS NOT NULL;

ALTER TABLE comments ADD COLUMN deleted_at DATETIME;
ALTER TABLE comments ADD COLUMN deleted_by INTEGER REFERENCES users(id);
CREATE INDEX idx_comments_deleted_at ON comments(deleted_at) WHERE deleted_at IS NOT NULL;