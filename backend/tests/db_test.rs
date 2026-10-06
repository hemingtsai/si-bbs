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
            "comments",
            "forum_comments",
            "forum_likes",
            "forum_posts",
            "forum_rules",
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

#[tokio::test]
async fn trash_view_aggregates_all_kinds() {
    let pool = common::test_pool().await;
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM trash_view ORDER BY kind")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(kinds.is_empty());
}
