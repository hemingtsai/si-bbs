//! Global search: FTS5 for real queries, escaped LIKE for very short ones.

mod common;

use axum::http::StatusCode;
use common::{register_and_login, test_ctx};

async fn create_wiki(
    server: &axum_test::TestServer,
    token: &str,
    title: &str,
    content: &str,
) -> i64 {
    server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "title": title, "category": "c", "content": content, "status": "published",
        }))
        .await
        .assert_status(StatusCode::CREATED)
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap()
}

async fn create_post(
    server: &axum_test::TestServer,
    token: &str,
    title: &str,
    content: &str,
) -> i64 {
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({"board": "life", "title": title, "content": content}))
        .await
        .assert_status(StatusCode::CREATED)
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn search_finds_content_across_all_three_sources() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    create_wiki(&server, &alice, "Rust 入门", "讲的是 rust 的所有权模型").await;
    create_post(&server, &alice, "聊 rust 工具链", "cargo 很好用").await;
    // A project, inserted directly: submitting would need a GitHub mock.
    sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, description, category, status, submitted_by) \
         VALUES ('rust-analyzer', 'https://github.com/rust-lang/rust-analyzer', 'rust-lang', \
                 'rust-analyzer', 'a rust language server', 'dev-tools', 'approved', 1)",
    )
    .execute(&_pool)
    .await
    .unwrap();

    let res = server.get("/api/search?q=rust").await;
    if res.status_code() != StatusCode::OK {
        panic!("search failed: {} {}", res.status_code(), res.text());
    }
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 3, "expected one hit per source: {body}");

    let kinds: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit["kind"].as_str().unwrap())
        .collect();
    // Ranked: the exact-name project should not be last, and every kind is present.
    for expected in ["wiki", "forum", "project"] {
        assert!(
            kinds.contains(&expected),
            "{expected} missing from {kinds:?}"
        );
    }
    // Excerpts come from the body, not the whole document.
    let wiki_hit = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["kind"] == "wiki")
        .unwrap();
    assert!(wiki_hit["excerpt"].as_str().unwrap().contains("rust"));
    assert!(wiki_hit["title"].as_str().unwrap().contains("Rust"));
}

#[tokio::test]
async fn search_works_on_chinese_substrings() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    create_post(&server, &alice, "随便标题", "这是一段关于模型量化的讨论").await;

    // A word tokenizer would treat the whole run of ideographs as one token, so this
    // is exactly the case the trigram tokenizer exists for.
    let res = server.get("/api/search?q=模型量化").await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    // A middle fragment works too.
    let res = server.get("/api/search?q=量化").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
}

#[tokio::test]
async fn very_short_queries_still_work_through_the_like_fallback() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    create_post(&server, &alice, "ab 标题", "正文").await;

    // Two characters: below the trigram minimum, so the LIKE branch answers.
    let res = server.get("/api/search?q=ab").await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
}

/// The old behaviour was `LIKE '%kw%'` with no escaping, so searching for `%`
/// returned the entire site.
#[tokio::test]
async fn wildcard_queries_are_treated_as_literal_text() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    create_post(&server, &alice, "普通标题", "普通正文").await;
    create_post(&server, &alice, "带%百分号", "50% 折扣").await;
    create_post(&server, &alice, "带_下划线", "a_b 命名").await;

    for (query, expected) in [("%", 1), ("_", 1), ("50%", 1), ("a_b", 1)] {
        let body = server
            .get(&format!("/api/search?q={query}"))
            .await
            .json::<serde_json::Value>();
        assert_eq!(
            body["total"], expected,
            "query {query:?} matched {} rows ({})",
            body["total"], body["items"]
        );
    }

    // A query that only matches by wildcard semantics gets nothing.
    let body = server
        .get("/api/search?q=zz%")
        .await
        .json::<serde_json::Value>();
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn search_only_returns_public_content() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    // A draft wiki page.
    server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "秘密草稿", "category": "c", "content": "机密内容", "status": "draft",
        }))
        .await
        .assert_status(StatusCode::CREATED);
    // A deleted forum post.
    let post = create_post(&server, &alice, "将被删除", "删除的内容").await;
    server
        .delete(&format!("/api/forum/posts/{post}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    // A pending project.
    sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, description, category, status, submitted_by) \
         VALUES ('待审项目', 'https://github.com/o/pending', 'o', 'pending', 'secret', 'c', 'pending', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    for query in ["秘密草稿", "将被删除", "待审项目"] {
        let body = server
            .get(&format!("/api/search?q={query}"))
            .await
            .json::<serde_json::Value>();
        assert_eq!(body["total"], 0, "{query} leaked: {body}");
    }
}

#[tokio::test]
async fn the_kind_filter_and_paging_work_and_bad_input_is_rejected() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    create_wiki(&server, &alice, "共享关键词 wiki", "共享关键词").await;
    create_post(&server, &alice, "共享关键词 帖子", "共享关键词").await;

    let only_forum = server
        .get("/api/search?q=共享关键词&kind=forum")
        .await
        .json::<serde_json::Value>();
    assert_eq!(only_forum["total"], 1);
    assert_eq!(only_forum["items"][0]["kind"], "forum");

    let all = server
        .get("/api/search?q=共享关键词&kind=all")
        .await
        .json::<serde_json::Value>();
    assert_eq!(all["total"], 2);

    // Paging.
    let first = server
        .get("/api/search?q=共享关键词&per_page=1")
        .await
        .json::<serde_json::Value>();
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["per_page"], 1);
    let second = server
        .get("/api/search?q=共享关键词&per_page=1&page=2")
        .await
        .json::<serde_json::Value>();
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    // ids are per-table, so two different sources can both be id 1: compare the pair.
    let first_key = (
        first["items"][0]["kind"].as_str().unwrap().to_string(),
        first["items"][0]["id"].as_i64().unwrap(),
    );
    let second_key = (
        second["items"][0]["kind"].as_str().unwrap().to_string(),
        second["items"][0]["id"].as_i64().unwrap(),
    );
    assert_ne!(first_key, second_key, "page 2 repeated a row from page 1");

    // An empty query is an empty page, not an error.
    let empty = server.get("/api/search?q=").await;
    empty.assert_status_ok();
    assert_eq!(empty.json::<serde_json::Value>()["total"], 0);

    server
        .get("/api/search?q=rust&kind=users")
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}

/// The index is maintained by triggers, so a later edit is searchable and a rewrite
/// stops matching the old text.
#[tokio::test]
async fn the_index_follows_edits() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let id = create_wiki(&server, &alice, "初始标题", "初始正文 about widgets").await;

    assert_eq!(
        server
            .get("/api/search?q=widgets")
            .await
            .json::<serde_json::Value>()["total"],
        1
    );

    server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "改过的标题", "category": "c", "content": "改过的正文 about sprockets",
            "status": "published",
        }))
        .await
        .assert_status_ok();

    // New text is found…
    assert_eq!(
        server
            .get("/api/search?q=sprockets")
            .await
            .json::<serde_json::Value>()["total"],
        1
    );
    // …and the removed text is not.
    assert_eq!(
        server
            .get("/api/search?q=widgets")
            .await
            .json::<serde_json::Value>()["total"],
        0,
        "a stale index entry survived the update"
    );
}
