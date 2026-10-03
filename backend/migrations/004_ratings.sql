CREATE TABLE ratings (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    user_id    INTEGER NOT NULL REFERENCES users(id),
    score      INTEGER NOT NULL CHECK(score BETWEEN 1 AND 10),
    comment    TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(project_id, user_id)
);

CREATE INDEX idx_ratings_project ON ratings(project_id);