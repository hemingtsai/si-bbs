-- The like/reply counters on forum_posts and forum_comments were denormalised and
-- maintained by hand (`SET likes_count = likes_count + 1`). Two concurrent
-- toggles could both apply the increment while only one like row was actually
-- inserted, so the stored number drifted away from the rows in forum_likes and
-- stayed wrong forever.
--
-- The counts are now derived with a subquery in the read queries, which makes
-- forum_likes and forum_comments the single source of truth. Keeping the old
-- columns around would leave a second, silently wrong answer sitting in the
-- table, so drop them.
ALTER TABLE forum_posts DROP COLUMN likes_count;
ALTER TABLE forum_posts DROP COLUMN comments_count;
ALTER TABLE forum_comments DROP COLUMN likes_count;

-- Counting the likes of one target cannot use the (user_id, target_kind,
-- target_id) primary key, so the derived count needs its own index.
CREATE INDEX idx_forum_likes_target ON forum_likes(target_kind, target_id);
