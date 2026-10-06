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

/// History: every save is a snapshot, and a stale editor cannot silently overwrite
/// someone else's work.
#[tokio::test]
async fn every_save_is_recorded_and_stale_edits_are_refused() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let auth = format!("Bearer {alice}");

    let created = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "版本页面", "category": "c", "content": "v1", "status": "published",
        }))
        .await;
    created.assert_status(axum::http::StatusCode::CREATED);
    let page = created.json::<serde_json::Value>();
    let id = page["id"].as_i64().unwrap();
    assert_eq!(page["revision"], 1, "creation must be revision 1");

    // Saving without base_revision works (read-then-write) and bumps the revision.
    let updated = server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "版本页面", "category": "c", "content": "v2", "status": "published",
        }))
        .await;
    updated.assert_status_ok();
    assert_eq!(updated.json::<serde_json::Value>()["revision"], 2);

    // An editor who started from revision 1 is now stale.
    let stale = server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "版本页面", "category": "c", "content": "v1-edited",
            "status": "published", "base_revision": 1,
        }))
        .await;
    stale.assert_status(axum::http::StatusCode::CONFLICT);
    // …and nothing was written.
    let detail = server
        .get(&format!("/api/wiki/page/{id}/revisions/2"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(detail["content"], "v2");

    // Up-to-date editors succeed.
    let ok = server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "版本页面", "category": "c", "content": "v3",
            "status": "published", "base_revision": 2, "comment": "小修",
        }))
        .await;
    ok.assert_status_ok();
    assert_eq!(ok.json::<serde_json::Value>()["revision"], 3);

    // History is newest-first metadata only.
    let history = server
        .get(&format!("/api/wiki/page/{id}/revisions"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(history["total"], 3);
    let items = history["items"].as_array().unwrap();
    assert_eq!(items[0]["revision_no"], 3);
    assert_eq!(items[0]["comment"], "小修");
    assert_eq!(items[1]["revision_no"], 2);
    assert_eq!(items[2]["revision_no"], 1);
    assert!(
        items[0]["content"].is_null(),
        "history must not ship bodies"
    );
    assert_eq!(items[2]["author_username"], "alice");

    // A single revision carries the body.
    let first = server
        .get(&format!("/api/wiki/page/{id}/revisions/1"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(first["content"], "v1");
    server
        .get(&format!("/api/wiki/page/{id}/revisions/99"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reverting_appends_a_new_revision_instead_of_rewriting_history() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let auth = format!("Bearer {alice}");
    let id = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "回滚页面", "category": "c", "content": "第一版", "status": "published",
        }))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    for content in ["第二版", "第三版"] {
        server
            .put(&format!("/api/wiki/page/{id}"))
            .add_header("Authorization", auth.clone())
            .json(&serde_json::json!({
                "title": "回滚页面", "category": "c", "content": content, "status": "published",
            }))
            .await
            .assert_status_ok();
    }

    let reverted = server
        .post(&format!("/api/wiki/page/{id}/revert/1"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({}))
        .await;
    reverted.assert_status_ok();
    let page = reverted.json::<serde_json::Value>();
    assert_eq!(page["revision"], 4, "a revert is a new revision");
    assert_eq!(page["content"], "第一版");

    // Revision 3 is still there, untouched.
    let third = server
        .get(&format!("/api/wiki/page/{id}/revisions/3"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(third["content"], "第三版");
    let fourth = server
        .get(&format!("/api/wiki/page/{id}/revisions/4"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(fourth["content"], "第一版");
    assert_eq!(fourth["comment"], "revert to revision 1");

    // Reverting from a stale base is refused, and a missing revision is a 404.
    server
        .post(&format!("/api/wiki/page/{id}/revert/4"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({"base_revision": 1}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);
    server
        .post(&format!("/api/wiki/page/{id}/revert/99"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revisions_of_a_draft_are_private_but_a_published_page_history_is_public() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let auth = format!("Bearer {alice}");

    let draft = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({"title":"草稿页","category":"c","content":"x"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    // Strangers cannot see a draft's history…
    server
        .get(&format!("/api/wiki/page/{draft}/revisions"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    // …the author can.
    server
        .get(&format!("/api/wiki/page/{draft}/revisions"))
        .add_header("Authorization", auth.clone())
        .await
        .assert_status_ok();

    let published = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title":"公开页","category":"c","content":"x","status":"published",
        }))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .get(&format!("/api/wiki/page/{published}/revisions"))
        .await
        .assert_status_ok();
}

/// A wiki URL is identity: renaming must not break the old one, and the old one
/// must not be handed to somebody else.
#[tokio::test]
async fn a_page_slug_can_be_changed_and_the_old_url_still_resolves() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let auth = format!("Bearer {alice}");

    // A Chinese title used to mean an unreadable `page-<millis>` URL and no way to
    // fix it; now the slug can be set explicitly.
    let created = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "中文标题", "category": "c", "content": "正文",
            "status": "published", "slug": "zhongwen-biaoti",
        }))
        .await;
    created.assert_status(axum::http::StatusCode::CREATED);
    let id = created.json::<serde_json::Value>()["id"].as_i64().unwrap();
    assert_eq!(
        created.json::<serde_json::Value>()["slug"],
        "zhongwen-biaoti"
    );

    // The same slug cannot be taken by another page.
    server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "另一个", "category": "c", "content": "x", "slug": "zhongwen-biaoti",
        }))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    // Rename it.
    let renamed = server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "中文标题", "category": "c", "content": "正文",
            "status": "published", "slug": "renamed-page",
        }))
        .await;
    renamed.assert_status_ok();
    assert_eq!(renamed.json::<serde_json::Value>()["slug"], "renamed-page");

    // The new URL works…
    let by_new = server.get("/api/wiki/renamed-page").await;
    by_new.assert_status_ok();
    assert_eq!(by_new.json::<serde_json::Value>()["id"], id);
    // …and so does the old one, answering with the canonical slug so a client can
    // rewrite its URL.
    let by_old = server.get("/api/wiki/zhongwen-biaoti").await;
    by_old.assert_status_ok();
    let body = by_old.json::<serde_json::Value>();
    assert_eq!(body["id"], id);
    assert_eq!(body["slug"], "renamed-page");

    // A new page cannot steal the retired slug and shadow the redirect.
    server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "抢注", "category": "c", "content": "x", "slug": "zhongwen-biaoti",
        }))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);
    let still = server
        .get("/api/wiki/zhongwen-biaoti")
        .await
        .json::<serde_json::Value>();
    assert_eq!(still["id"], id);
}

#[tokio::test]
async fn slugs_that_would_shadow_a_route_or_break_the_url_are_refused() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let auth = format!("Bearer {alice}");

    for bad in [
        "mine",
        "categories",
        "page",
        "new",
        "-lead",
        "trail-",
        "has space",
        "有中文",
        "a/b",
    ] {
        server
            .post("/api/wiki")
            .add_header("Authorization", auth.clone())
            .json(&serde_json::json!({
                "title": "标题", "category": "c", "content": "x", "slug": bad,
            }))
            .await
            .assert_status(axum::http::StatusCode::BAD_REQUEST);
    }
    // Upper case is normalised, not rejected.
    let ok = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "标题", "category": "c", "content": "x", "slug": "Mixed-Case",
        }))
        .await;
    ok.assert_status(axum::http::StatusCode::CREATED);
    assert_eq!(ok.json::<serde_json::Value>()["slug"], "mixed-case");
}

/// The alias table references the page, so purging a renamed page from the bin must
/// still work (the same foreign-key trap the purge path already handles elsewhere).
#[tokio::test]
async fn purging_a_renamed_page_cleans_up_its_aliases() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let admin = register_login_as_role(&server, &pool, "root", "admin").await;
    let auth = format!("Bearer {alice}");

    let id = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "要删的", "category": "c", "content": "x",
            "status": "published", "slug": "old-name",
        }))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "要删的", "category": "c", "content": "x",
            "status": "published", "slug": "new-name",
        }))
        .await
        .assert_status_ok();
    server
        .delete(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", auth.clone())
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    server
        .delete(&format!("/api/trash/wiki/{id}"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let aliases: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wiki_slug_aliases")
        .fetch_one(&pool)
        .await
        .unwrap();
    let revisions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wiki_revisions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        (aliases, revisions),
        (0, 0),
        "cascade must clean both tables"
    );
}

/// A diff is only useful if it is exact: applying it must reproduce the target.
#[tokio::test]
async fn diffing_two_revisions_reports_the_change() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let auth = format!("Bearer {alice}");

    let id = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "差异页面", "category": "c", "status": "published",
            "content": "第一行\n第二行\n第三行",
        }))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .put(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "title": "差异页面", "category": "c", "status": "published",
            "content": "第一行\n改过的第二行\n第三行\n新增的一行",
        }))
        .await
        .assert_status_ok();

    // Default `to` is the current revision, so `?from=1` is "the latest save".
    let res = server
        .get(&format!("/api/wiki/page/{id}/diff?from=1"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["from"], 1);
    assert_eq!(body["to"], 2);
    assert_eq!(body["coarse"], false);
    assert_eq!(body["added"], 2);
    assert_eq!(body["removed"], 1);

    let lines = body["lines"].as_array().unwrap();
    // Unchanged lines keep both numbers; the change is marked.
    assert_eq!(lines[0]["kind"], "same");
    assert_eq!(lines[0]["text"], "第一行");
    assert_eq!(
        (lines[0]["old_no"].as_i64(), lines[0]["new_no"].as_i64()),
        (Some(1), Some(1))
    );
    let removed: Vec<&str> = lines
        .iter()
        .filter(|l| l["kind"] == "remove")
        .map(|l| l["text"].as_str().unwrap())
        .collect();
    let added: Vec<&str> = lines
        .iter()
        .filter(|l| l["kind"] == "add")
        .map(|l| l["text"].as_str().unwrap())
        .collect();
    assert_eq!(removed, ["第二行"]);
    assert_eq!(added, ["改过的第二行", "新增的一行"]);

    // Explicit ranges work, identical revisions are refused, and a missing
    // revision is a 404.
    server
        .get(&format!("/api/wiki/page/{id}/diff?from=1&to=2"))
        .await
        .assert_status_ok();
    server
        .get(&format!("/api/wiki/page/{id}/diff?from=2&to=2"))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    server
        .get(&format!("/api/wiki/page/{id}/diff?from=1&to=99"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    server
        .get(&format!("/api/wiki/page/{id}/diff?from=0"))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

/// A draft's history and diffs are as private as the draft itself.
#[tokio::test]
async fn diffing_a_draft_requires_the_author() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let auth = format!("Bearer {alice}");
    let id = server
        .post("/api/wiki")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({"title":"草稿差异","category":"c","content":"a"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    server
        .get(&format!("/api/wiki/page/{id}/diff?from=1&to=1"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    server
        .get(&format!("/api/wiki/page/{id}/diff?from=1&to=1"))
        .add_header("Authorization", auth.clone())
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST); // visible, just a silly range
}

#[tokio::test]
async fn wiki_lists_omit_bodies_but_details_keep_them() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let body = "内容".repeat(500);
    let created = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "长正文页面", "category": "c", "content": body, "status": "published",
        }))
        .await;
    let page = created.json::<serde_json::Value>();
    let slug = page["slug"].as_str().unwrap().to_string();
    assert_eq!(page["content"], body, "creation echoes the body");

    for url in ["/api/wiki", "/api/wiki/mine"] {
        let res = server
            .get(url)
            .add_header("Authorization", format!("Bearer {alice}"))
            .await;
        res.assert_status_ok();
        let item = res.json::<serde_json::Value>()["items"][0].clone();
        assert!(item.get("content").is_none(), "{url} shipped the body");
        assert!(item["excerpt"].as_str().unwrap().chars().count() <= 160);
    }

    // Detail keeps it.
    let detail = server
        .get(&format!("/api/wiki/{slug}"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(detail["content"], body);
}
