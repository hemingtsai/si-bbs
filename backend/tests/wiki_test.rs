mod common;

use axum_test::TestServer;
use common::{register_and_login, register_login_as_role, test_ctx};
use sqlx::SqlitePool;

async fn create_page(
    server: &TestServer,
    token: &str,
    title: &str,
    category: &str,
    status: &str,
) -> axum_test::TestResponse {
    server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "title": title,
            "category": category,
            "content": "# heading\n\nbody text",
            "status": status,
        }))
        .await
}

/// Publish a page as `author`, registering that user once per test.
async fn approved_page(server: &TestServer, title: &str) -> i64 {
    let token = register_and_login(server, "author").await;
    let res = create_page(server, &token, title, "guides", "published").await;
    res.assert_status(axum::http::StatusCode::CREATED);
    res.json::<serde_json::Value>()["id"].as_i64().unwrap()
}

#[test]
fn slugify_handles_ascii_and_non_ascii_titles() {
    use si_bbs_backend::models::wiki::slugify;
    assert_eq!(slugify("Getting Started"), "getting-started");
    assert_eq!(slugify("  Docker & Compose  "), "docker-compose");
    assert_eq!(slugify("Rust 101"), "rust-101");
    assert_eq!(slugify("a/b\\c"), "a-b-c");
    // Non-ASCII titles cannot be transliterated, so they get a unique fallback.
    let chinese = slugify("快速开始");
    assert!(chinese.starts_with("page-"), "got {chinese}");
}

#[tokio::test]
async fn create_page_returns_201_with_slug_and_author() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let res = create_page(&server, &alice, "Getting Started", "guides", "published").await;
    res.assert_status(axum::http::StatusCode::CREATED);
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["slug"], "getting-started");
    assert_eq!(body["title"], "Getting Started");
    assert_eq!(body["category"], "guides");
    assert_eq!(body["status"], "published");
    assert_eq!(body["author_username"], "alice");
    // Content stays raw Markdown for client-side rendering.
    assert_eq!(body["content"], "# heading\n\nbody text");
}

#[tokio::test]
async fn duplicate_titles_get_unique_slugs() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let first = create_page(&server, &alice, "Same Title", "guides", "published").await;
    first.assert_status(axum::http::StatusCode::CREATED);
    assert_eq!(first.json::<serde_json::Value>()["slug"], "same-title");

    let second = create_page(&server, &alice, "Same Title", "guides", "published").await;
    second.assert_status(axum::http::StatusCode::CREATED);
    assert_eq!(second.json::<serde_json::Value>()["slug"], "same-title-2");
}

#[tokio::test]
async fn create_page_requires_login_and_valid_input() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    server
        .post("/api/wiki")
        .json(&serde_json::json!({
            "title": "x", "category": "guides", "content": "y",
        }))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);

    for (title, category, content, status) in [
        ("", "guides", "body", "published"),
        ("  ", "guides", "body", "published"),
        ("Title", "", "body", "published"),
        ("Title", "guides", "   ", "published"),
        ("Title", "guides", "body", "archived"),
    ] {
        server
            .post("/api/wiki")
            .add_header("Authorization", format!("Bearer {alice}"))
            .json(&serde_json::json!({
                "title": title, "category": category, "content": content, "status": status,
            }))
            .await
            .assert_status(axum::http::StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn page_is_draft_by_default() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let res = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "Draft Page", "category": "guides", "content": "wip",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::CREATED);
    assert_eq!(res.json::<serde_json::Value>()["status"], "draft");
}

#[tokio::test]
async fn list_only_shows_published_pages() {
    let (server, _pool) = test_ctx().await;
    let id = approved_page(&server, "Public Page").await;
    let token = register_and_login(&server, "writer").await;
    create_page(&server, &token, "Hidden Page", "guides", "draft")
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let res = server.get("/api/wiki").await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["slug"], "public-page");
    let _ = id;
}

#[tokio::test]
async fn list_filters_by_category_and_keyword() {
    let (server, _pool) = test_ctx().await;
    approved_page(&server, "Docker Guide").await;
    let token = register_and_login(&server, "writer").await;
    create_page(&server, &token, "Rust Guide", "languages", "published")
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let res = server.get("/api/wiki?category=guides").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    let res = server.get("/api/wiki?category=languages").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    let res = server.get("/api/wiki?category=missing").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 0);

    let res = server.get("/api/wiki?q=rust").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    // Content is searched too, not only the title.
    let res = server.get("/api/wiki?q=body%20text").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 2);
}

#[tokio::test]
async fn categories_returns_published_counts() {
    let (server, _pool) = test_ctx().await;
    let token = register_and_login(&server, "author").await;
    create_page(&server, &token, "Page One", "guides", "published")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    create_page(&server, &token, "Page Two", "reference", "published")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    create_page(&server, &token, "Draft", "guides", "draft")
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let res = server.get("/api/wiki/categories").await;
    res.assert_status_ok();
    let arr = res.json::<serde_json::Value>();
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // Alphabetical, and drafts are excluded from the counts.
    assert_eq!(arr[0]["category"], "guides");
    assert_eq!(arr[0]["count"], 1);
    assert_eq!(arr[1]["category"], "reference");
    assert_eq!(arr[1]["count"], 1);
}

#[tokio::test]
async fn published_page_is_publicly_readable() {
    let (server, _pool) = test_ctx().await;
    approved_page(&server, "Public Page").await;

    let res = server.get("/api/wiki/public-page").await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["title"], "Public Page");

    server
        .get("/api/wiki/does-not-exist")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn draft_page_is_hidden_from_strangers() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    create_page(&server, &alice, "Secret Draft", "guides", "draft")
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Anonymous and other users get a 404, not a 403, so drafts stay invisible.
    server
        .get("/api/wiki/secret-draft")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    server
        .get("/api/wiki/secret-draft")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // The author and staff can read it.
    server
        .get("/api/wiki/secret-draft")
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status_ok();
    server
        .get("/api/wiki/secret-draft")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn mine_lists_drafts_and_published_for_author_only() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;

    create_page(&server, &alice, "Alice Draft", "guides", "draft")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    create_page(&server, &alice, "Alice Live", "guides", "published")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    create_page(&server, &bob, "Bob Draft", "guides", "draft")
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let res = server
        .get("/api/wiki/mine")
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 2);

    let res = server
        .get("/api/wiki/mine")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    server
        .get("/api/wiki/mine")
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    let _ = pool;
}

#[tokio::test]
async fn update_requires_author_or_staff() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    let res = create_page(&server, &alice, "Editable", "guides", "published").await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    let body = serde_json::json!({
        "title": "Edited", "category": "guides", "content": "new body", "status": "published",
    });

    server
        .put(&format!("/api/wiki/page/{id}"))
        .json(&body)
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&body)
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    // Author edits.
    let res = server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&body)
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["content"], "new body");
    // The slug is stable across edits.
    assert_eq!(res.json::<serde_json::Value>()["slug"], "editable");

    // Staff edits too.
    let res = server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({
            "title": "Staff Edit", "category": "reference", "content": "staff", "status": "draft",
        }))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["status"], "draft");
}

#[tokio::test]
async fn update_missing_page_is_404() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    server
        .put("/api/wiki/page/999999")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "x", "category": "guides", "content": "y",
        }))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_is_soft_and_hides_the_page() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let res = create_page(&server, &alice, "Removable", "guides", "published").await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();

    let res = server
        .delete(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status(axum::http::StatusCode::NO_CONTENT);

    // Gone from the API...
    server
        .get("/api/wiki/removable")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    let res = server.get("/api/wiki").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 0);

    // ...but still recoverable from the trash, with the deleter recorded.
    let row: (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT deleted_at, deleted_by FROM wiki_pages WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(row.0.is_some());
    assert!(row.1.is_some());
}

#[tokio::test]
async fn delete_requires_permission() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    let res = create_page(&server, &alice, "Protected", "guides", "published").await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();

    server
        .delete(&format!("/api/wiki/page/{id}"))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    server
        .delete(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    server
        .delete("/api/wiki/page/999999")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // A moderator may remove anyone's page.
    server
        .delete(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
}

/// Reuse an existing pool handle; the wiki tests do not inspect it directly.
async fn _pool_or(server: &TestServer) -> SqlitePool {
    let _ = server;
    common::test_pool().await
}
