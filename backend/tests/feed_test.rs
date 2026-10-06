//! RSS feeds. They are public by design — a feed reader sends no auth header — so
//! the tests also pin what a stranger can see.

mod common;

use axum::http::StatusCode;
use common::{register_and_login, test_ctx};

fn body_of(res: axum_test::TestResponse) -> String {
    res.assert_status(StatusCode::OK);
    res.text()
}

#[tokio::test]
async fn forum_feed_lists_posts_with_absolute_links_and_escaped_titles() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    // A title that would break the XML if it were not escaped, and a control
    // character that XML cannot represent at all.
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "board": "models",
            "title": "标题 <b>& 符号\u{1}",
            "content": "正文 with <script>alert(1)</script>",
        }))
        .await
        .assert_status(StatusCode::CREATED);

    let res = server
        .get("/forum/feed.xml")
        .add_header("x-forwarded-proto", "https")
        .add_header("host", "sibbs.cn")
        .await;
    assert_eq!(
        res.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/rss+xml; charset=utf-8")
    );
    let xml = body_of(res);

    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
    assert!(xml.contains("<title>SI BBS · 论坛</title>"));
    // The proxied host is what the links are built from.
    assert!(xml.contains("<atom:link href=\"https://sibbs.cn/forum/feed.xml\" rel=\"self\""));
    assert!(
        xml.contains("<link>https://sibbs.cn/forum/1</link>"),
        "XML was:\n{xml}"
    );
    // Escaped, and the illegal control character is gone.
    assert!(xml.contains("标题 &lt;b&gt;&amp; 符号"));
    assert!(!xml.contains('\u{1}'));
    assert!(xml.contains("正文 with &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(xml.contains("<pubDate>"));
}

#[tokio::test]
async fn feeds_are_empty_but_valid_before_anything_exists() {
    let (server, _pool) = test_ctx().await;

    for path in [
        "/feed.xml",
        "/forum/feed.xml",
        "/wiki/feed.xml",
        "/projects/feed.xml",
    ] {
        let xml = body_of(server.get(path).await);
        assert!(xml.contains("<rss version=\"2.0\""), "{path} is not RSS");
        assert!(xml.ends_with("</rss>\n"), "{path} is truncated");
        assert!(!xml.contains("<item>"), "{path} invented items");
    }
}

#[tokio::test]
async fn the_site_feed_mixes_sources_newest_first() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    // A wiki page, then a forum post: the post is newer and must come first.
    server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "旧页面", "category": "c", "content": "wiki body", "status": "published",
        }))
        .await
        .assert_status(StatusCode::CREATED);
    // Timestamps are second-resolution, so make the order explicit.
    sqlx::query("UPDATE wiki_pages SET updated_at = datetime('now', '-1 hour')")
        .execute(&pool)
        .await
        .unwrap();
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"board": "life", "title": "新帖子", "content": "post body"}))
        .await
        .assert_status(StatusCode::CREATED);

    let xml = body_of(server.get("/feed.xml").await);
    let post_at = xml.find("新帖子").expect("post missing");
    let wiki_at = xml.find("旧页面").expect("wiki missing");
    assert!(post_at < wiki_at, "site feed is not newest-first");
}

#[tokio::test]
async fn wiki_feed_only_exposes_published_pages() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    for (title, status) in [("已发布", "published"), ("草稿", "draft")] {
        server
            .post("/api/wiki")
            .add_header("Authorization", format!("Bearer {alice}"))
            .json(&serde_json::json!({
                "title": title, "category": "c", "content": "body", "status": status,
            }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    let xml = body_of(server.get("/wiki/feed.xml").await);
    assert!(xml.contains("已发布"));
    assert!(
        !xml.contains("草稿"),
        "a draft leaked into a public feed: {xml}"
    );
}

#[tokio::test]
async fn a_board_filter_narrows_the_forum_feed_and_rejects_nonsense() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    for (board, title) in [("models", "模型帖"), ("life", "闲聊帖")] {
        server
            .post("/api/forum/posts")
            .add_header("Authorization", format!("Bearer {alice}"))
            .json(&serde_json::json!({"board": board, "title": title, "content": "x"}))
            .await
            .assert_status(StatusCode::CREATED);
    }

    let xml = body_of(server.get("/forum/feed.xml?board=models").await);
    assert!(xml.contains("模型帖"));
    assert!(!xml.contains("闲聊帖"), "board filter leaked another board");

    server
        .get("/forum/feed.xml?board=nope")
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn excluded_content_never_reaches_a_feed() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let post_id = server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"board": "life", "title": "将被删除", "content": "x"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .delete(&format!("/api/forum/posts/{post_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // A pending project is not public anywhere else, so it must not be here either.
    sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, category, status, submitted_by) \
         VALUES ('待审项目', 'https://github.com/o/pending', 'o', 'pending', 'c', 'pending', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    for path in ["/feed.xml", "/forum/feed.xml", "/projects/feed.xml"] {
        let xml = body_of(server.get(path).await);
        assert!(!xml.contains("将被删除"), "deleted post in {path}");
        assert!(!xml.contains("待审项目"), "pending project in {path}");
    }
}

#[tokio::test]
async fn configured_public_base_url_wins_over_proxy_headers() {
    let pool = common::test_pool().await;
    let mut cfg = common::test_config("https://api.github.com");
    cfg.public_base_url = "https://configured.example/".into();
    let server = axum_test::TestServer::new(si_bbs_backend::create_router(
        si_bbs_backend::routes::AppState::new(pool, cfg),
    ));

    let xml = body_of(
        server
            .get("/forum/feed.xml")
            .add_header("host", "attacker.test")
            .await,
    );
    assert!(xml.contains("https://configured.example/forum/feed.xml"));
    assert!(!xml.contains("attacker.test"));
}

/// The wiki links must use the slug: `/api/wiki/{slug}` is the only detail route,
/// so an id-based link in a feed is a 404 for every subscriber.
#[tokio::test]
async fn wiki_links_use_the_slug_in_both_feeds() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "Readable Slug", "category": "c", "content": "body", "status": "published",
        }))
        .await
        .assert_status(StatusCode::CREATED);

    for path in ["/wiki/feed.xml", "/feed.xml"] {
        let xml = body_of(server.get(path).await);
        assert!(
            xml.contains("<link>http://localhost/wiki/readable-slug</link>"),
            "{path} did not link by slug:\n{xml}"
        );
    }
}
