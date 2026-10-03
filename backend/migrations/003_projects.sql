CREATE TABLE projects (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    github_url   TEXT NOT NULL UNIQUE,
    owner        TEXT NOT NULL,
    repo         TEXT NOT NULL,
    description  TEXT,
    readme_raw   TEXT,
    language     TEXT,
    stars        INTEGER DEFAULT 0,
    forks        INTEGER DEFAULT 0,
    license      TEXT,
    topics       TEXT,
    category     TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    submitted_by INTEGER NOT NULL REFERENCES users(id),
    reviewed_by  INTEGER REFERENCES users(id),
    review_note  TEXT,
    created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_projects_status ON projects(status);
CREATE INDEX idx_projects_category ON projects(category);
CREATE INDEX idx_projects_stars ON projects(stars DESC);