-- Independent forum boards: flat posts + comments, likes, per-board rules.
-- Boards are a fixed enum; posts and replies need no moderation review.
CREATE TABLE forum_posts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    board         TEXT NOT NULL CHECK(board IN ('models','tools','life')),
    title         TEXT NOT NULL,
    content       TEXT NOT NULL,
    author_id     INTEGER NOT NULL REFERENCES users(id),
    is_featured   INTEGER NOT NULL DEFAULT 0,
    likes_count   INTEGER NOT NULL DEFAULT 0,
    comments_count INTEGER NOT NULL DEFAULT 0,
    deleted_at    DATETIME,
    deleted_by    INTEGER REFERENCES users(id),
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_forum_posts_board ON forum_posts(board, deleted_at, is_featured DESC, created_at DESC);
CREATE INDEX idx_forum_posts_author ON forum_posts(author_id, deleted_at);

CREATE TABLE forum_comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id    INTEGER NOT NULL REFERENCES forum_posts(id),
    author_id  INTEGER NOT NULL REFERENCES users(id),
    content    TEXT NOT NULL,
    likes_count INTEGER NOT NULL DEFAULT 0,
    deleted_at DATETIME,
    deleted_by INTEGER REFERENCES users(id),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_forum_comments_post ON forum_comments(post_id, deleted_at);

-- One like per user per target (post or comment). PRIMARY KEY makes it idempotent.
CREATE TABLE forum_likes (
    user_id     INTEGER NOT NULL REFERENCES users(id),
    target_kind TEXT NOT NULL CHECK(target_kind IN ('post','comment')),
    target_id   INTEGER NOT NULL,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, target_kind, target_id)
);

-- Site rules. `board` is either one of the three boards or 'global'.
CREATE TABLE forum_rules (
    board      TEXT PRIMARY KEY CHECK(board IN ('global','models','tools','life')),
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    updated_by INTEGER REFERENCES users(id),
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO forum_rules (board, title, content) VALUES
  ('global', '总站规', '# 总站规\n\n1. 友善交流，禁止人身攻击。\n2. 模型讨论请注明模型名称与版本。\n3. 工具交流请附上使用场景与必要链接。\n4. 谈天说地请自觉下限，聊点有营养的。'),
  ('models', '模型讨论板规', '# 模型讨论板规\n\n讨论请注明：模型名称、版本、训练/推理环境、评测方式。'),
  ('tools',  '工具交流板规', '# 工具交流板规\n\n分享工具时请说明平台、依赖与最小可行复现步骤。'),
  ('life',   '谈天说地板规', '# 谈天说地板规\n\n闲聊也请尊重他人，禁止广告与引战。');
