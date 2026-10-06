-- Extend trash_view with forum content so deleted posts/comments flow
-- through the same restore/purge pipeline as the other kinds.
DROP VIEW trash_view;
CREATE VIEW trash_view AS
SELECT 'wiki'        AS kind, id, title AS name, deleted_at, deleted_by FROM wiki_pages      WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'project'     AS kind, id, name  AS name, deleted_at, deleted_by FROM projects        WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'comment'     AS kind, id, substr(content, 1, 50) AS name, deleted_at, deleted_by FROM comments WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'forum_post'  AS kind, id, title AS name, deleted_at, deleted_by FROM forum_posts     WHERE deleted_at IS NOT NULL
UNION ALL
SELECT 'forum_comment' AS kind, id, substr(content, 1, 50) AS name, deleted_at, deleted_by FROM forum_comments WHERE deleted_at IS NOT NULL;
