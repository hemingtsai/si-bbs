mod common;

#[tokio::test]
async fn migrations_apply_cleanly() {
    let pool = common::test_pool().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        vec![
            "audit_log",
            "comments",
            "content_reports",
            "forum_comments",
            "forum_likes",
            "forum_posts",
            "forum_rules",
            "password_resets",
            "projects",
            "ratings",
            "users",
            "wiki_pages"
        ]
    );
}

#[tokio::test]
async fn soft_delete_columns_exist() {
    let pool = common::test_pool().await;
    for (table, columns) in [
        ("wiki_pages", vec!["deleted_at", "deleted_by"]),
        (
            "projects",
            vec!["deleted_at", "deleted_by", "readme_fetched_at"],
        ),
        ("comments", vec!["deleted_at", "deleted_by"]),
        ("forum_posts", vec!["deleted_at", "deleted_by"]),
        ("forum_comments", vec!["deleted_at", "deleted_by"]),
    ] {
        let cols: Vec<String> =
            sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .fetch_all(&pool)
                .await
                .unwrap();
        for c in columns {
            assert!(cols.contains(&c.to_string()), "{table} missing {c}");
        }
    }
}

/// The counters used to be real columns; they are derived now, and a stale copy
/// must not come back.
#[tokio::test]
async fn forum_count_columns_are_gone() {
    let pool = common::test_pool().await;
    for table in ["forum_posts", "forum_comments"] {
        let cols: Vec<String> =
            sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            !cols.iter().any(|c| c.ends_with("_count")),
            "{table} still stores a count column: {cols:?}"
        );
    }
}

/// The in-memory harness must enforce foreign keys exactly like production, or a
/// constraint bug (such as the purge cascade) can never surface in a test. WAL is
/// deliberately not covered: `:memory:` databases have no separate journal file,
/// so those semantics only exist on the file-backed deployment.
#[tokio::test]
async fn the_test_harness_enforces_foreign_keys() {
    let pool = common::test_pool().await;

    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1, "the test pool must run with foreign keys on");

    sqlx::query("INSERT INTO users (username, email, password_hash) VALUES ('u','u@x','h')")
        .execute(&pool)
        .await
        .unwrap();
    // A comment on a project that does not exist must be rejected.
    let err =
        sqlx::query("INSERT INTO comments (project_id, user_id, content) VALUES (999, 1, 'x')")
            .execute(&pool)
            .await;
    assert!(err.is_err(), "a dangling foreign key was accepted");
}

#[tokio::test]
async fn trash_view_aggregates_all_kinds() {
    let pool = common::test_pool().await;

    // Nothing deleted yet: the view must not invent rows.
    let empty: Vec<String> = sqlx::query_scalar("SELECT kind FROM trash_view")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(empty.is_empty());

    sqlx::query("INSERT INTO users (username, email, password_hash) VALUES ('u','u@x','h')")
        .execute(&pool)
        .await
        .unwrap();
    // One soft-deleted row of every kind the view claims to union.
    for sql in [
        "INSERT INTO wiki_pages (title, slug, category, content, author_id, deleted_at) \
         VALUES ('w','w','c','x',1,CURRENT_TIMESTAMP)",
        "INSERT INTO projects (name, github_url, owner, repo, category, submitted_by, deleted_at) \
         VALUES ('p','https://github.com/o/r','o','r','c',1,CURRENT_TIMESTAMP)",
        "INSERT INTO comments (project_id, user_id, content, deleted_at) \
         VALUES (1,1,'x',CURRENT_TIMESTAMP)",
        "INSERT INTO forum_posts (board, title, content, author_id, deleted_at) \
         VALUES ('life','t','c',1,CURRENT_TIMESTAMP)",
        "INSERT INTO forum_comments (post_id, author_id, content, deleted_at) \
         VALUES (1,1,'x',CURRENT_TIMESTAMP)",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }

    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM trash_view ORDER BY kind")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(
        kinds,
        ["comment", "forum_comment", "forum_post", "project", "wiki"]
    );

    // Restoring a row must take it back out of the bin.
    sqlx::query("UPDATE comments SET deleted_at = NULL")
        .execute(&pool)
        .await
        .unwrap();
    let after: Vec<String> = sqlx::query_scalar("SELECT kind FROM trash_view")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        !after.iter().any(|k| k == "comment"),
        "a live comment is still listed in the bin: {after:?}"
    );
}
