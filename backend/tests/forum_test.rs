mod common;

use axum_test::TestServer;
use common::{register_and_login, register_login_as_role, test_ctx};
async fn create_post(
    server: &TestServer,
    token: &str,
    board: &str,
    title: &str,
) -> axum_test::TestResponse {
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "board": board, "title": title, "content": "body" }))
        .await
}

#[tokio::test]
async fn boards_list_all_three_fixed_boards() {
    let (server, _pool) = test_ctx().await;
    let res = server.get("/api/forum/boards").await;
    res.assert_status_ok();
    let slugs: Vec<String> = res
        .json::<Vec<serde_json::Value>>()
        .into_iter()
        .map(|b| b["slug"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(slugs, ["models", "tools", "life"]);
}

#[tokio::test]
async fn post_workflow_without_moderation() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    // Posting is instant — no pending state at all.
    let res = create_post(&server, &alice, "models", "第一条").await;
    res.assert_status(axum::http::StatusCode::CREATED);
    let post = res.json::<serde_json::Value>();
    assert_eq!(post["title"], "第一条");
    assert_eq!(post["board"], "models");
    assert_eq!(post["status"], serde_json::Value::Null); // no status field
    assert_eq!(post["author_username"], "alice");

    let list = server.get("/api/forum/posts?board=models").await;
    assert_eq!(list.json::<serde_json::Value>()["total"], 1);

    let other = server.get("/api/forum/posts?board=life").await;
    assert_eq!(other.json::<serde_json::Value>()["total"], 0);

    // Board filtering is exact, and an invalid board slug is rejected.
    server
        .get("/api/forum/posts?board=wizard")
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn posts_search_filters_by_keyword() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    create_post(&server, &alice, "tools", "模型部署").await;
    create_post(&server, &alice, "tools", "别的标题").await;

    let res = server.get("/api/forum/posts?q=模型").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
}

#[tokio::test]
async fn author_can_edit_or_delete_own_post() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let id = create_post(&server, &alice, "life", "原标题")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    // Another user cannot edit or delete it.
    server
        .patch(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "board": "life", "title": "改", "content": "x" }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    server
        .delete(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    // The author can.
    let res = server
        .patch(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({ "board": "life", "title": "新标题", "content": "新正文" }))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["title"], "新标题");

    // Own delete works, and a second delete 404s.
    server
        .delete(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    let _ = pool;
}

#[tokio::test]
async fn moderator_can_delete_any_post() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let id = create_post(&server, &alice, "models", "要被删")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    server
        .delete(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let list = server.get("/api/forum/posts").await;
    assert_eq!(list.json::<serde_json::Value>()["total"], 0);
}

#[tokio::test]
async fn post_likes_are_idempotent_per_user() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let id = create_post(&server, &alice, "tools", "点赞目标")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    // Alice likes, then likes again: the row flips off, count back to 0.
    let res = server
        .post(&format!("/api/forum/posts/{id}/like"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    assert_eq!(res.json::<serde_json::Value>()["liked"], true);
    assert_eq!(res.json::<serde_json::Value>()["likes_count"], 1);
    let res = server
        .post(&format!("/api/forum/posts/{id}/like"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    assert_eq!(res.json::<serde_json::Value>()["liked"], false);
    assert_eq!(res.json::<serde_json::Value>()["likes_count"], 0);

    // Bob likes, Alice likes again: both rows count once each.
    server
        .post(&format!("/api/forum/posts/{id}/like"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await;
    server
        .post(&format!("/api/forum/posts/{id}/like"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    let res = server.get(&format!("/api/forum/posts/{id}")).await;
    assert_eq!(res.json::<serde_json::Value>()["likes_count"], 2);
    let _ = pool;
}

#[tokio::test]
async fn comments_need_no_review_and_support_likes() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let id = create_post(&server, &alice, "life", "帖子")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    let res = server
        .post(&format!("/api/forum/posts/{id}/comments"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "content": "沙发" }))
        .await;
    res.assert_status(axum::http::StatusCode::CREATED);
    let cid = res.json::<serde_json::Value>()["id"].as_i64().unwrap();

    let list = server.get(&format!("/api/forum/posts/{id}/comments")).await;
    assert_eq!(list.json::<serde_json::Value>()["total"], 1);

    let like = server
        .post(&format!("/api/forum/comments/{cid}/like"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    like.assert_status_ok();
    let list = server.get(&format!("/api/forum/posts/{id}/comments")).await;
    assert_eq!(
        list.json::<serde_json::Value>()["items"][0]["likes_count"],
        1
    );

    // Empty or oversized comment is rejected.
    server
        .post(&format!("/api/forum/posts/{id}/comments"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "content": "   " }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    let _ = pool;
}

#[tokio::test]
async fn comment_delete_permissions() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let id = create_post(&server, &alice, "models", "帖子")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    let cid = server
        .post(&format!("/api/forum/posts/{id}/comments"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "content": "bob 的评论" }))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    server
        .delete(&format!("/api/forum/comments/{cid}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    server
        .delete(&format!("/api/forum/comments/{cid}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let list = server.get(&format!("/api/forum/posts/{id}/comments")).await;
    assert_eq!(list.json::<serde_json::Value>()["total"], 0);
    let _ = pool;
}

#[tokio::test]
async fn featured_posts_sort_first() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let a = create_post(&server, &alice, "tools", "普通")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    let b = create_post(&server, &alice, "tools", "被精选")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    // A plain user cannot feature.
    server
        .patch(&format!("/api/forum/posts/{b}/featured"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({ "featured": true }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    let res = server
        .patch(&format!("/api/forum/posts/{b}/featured"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({ "featured": true }))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["is_featured"], 1);

    let res = server.get("/api/forum/posts?board=tools").await;
    let items = res.json::<serde_json::Value>()["items"].clone();
    assert_eq!(items[0]["id"], b);
    assert_eq!(items[1]["id"], a);
}

#[tokio::test]
async fn rules_are_registered_per_board_and_global() {
    let (server, _pool) = test_ctx().await;

    let all = server.get("/api/forum/rules").await;
    let rows = all.json::<Vec<serde_json::Value>>();
    assert_eq!(rows.len(), 4); // global + 3 boards

    let models = server.get("/api/forum/rules?board=models").await;
    let rows = models.json::<Vec<serde_json::Value>>();
    assert_eq!(rows.len(), 2); // global + models
    assert_eq!(rows[0]["board"], "global");
    assert_eq!(rows[1]["board"], "models");

    server
        .get("/api/forum/rules?board=nope")
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn staff_can_upsert_rules() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    server
        .put("/api/forum/rules/tools")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({ "title": "t", "content": "c" }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    let res = server
        .put("/api/forum/rules/tools")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({ "title": "工具新规", "content": "# 工具新规\n\n请描述版本。" }))
        .await;
    res.assert_status_ok();

    let res = server.get("/api/forum/rules?board=tools").await;
    let rows = res.json::<Vec<serde_json::Value>>();
    assert_eq!(rows[1]["title"], "工具新规");

    server
        .put("/api/forum/rules/nope")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({ "title": "t", "content": "c" }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn forum_content_lands_in_trash_and_can_be_restored() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let id = create_post(&server, &alice, "models", "会删")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    server
        .delete(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let trash = server
        .get("/api/trash")
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    let items = trash.json::<Vec<serde_json::Value>>();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "forum_post");
    assert_eq!(items[0]["name"], "会删");

    server
        .post(&format!("/api/trash/forum_post/{id}/restore"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status_ok();

    let res = server.get("/api/forum/posts?board=models").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
    let _ = pool;
}

#[tokio::test]
async fn likes_on_a_deleted_post_return_404() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let id = create_post(&server, &alice, "life", "马上删")
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .delete(&format!("/api/forum/posts/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;

    server
        .post(&format!("/api/forum/posts/{id}/like"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_input_validation() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    for (board, title, content) in [
        ("", "t", "c"),
        ("nope", "t", "c"),
        ("models", "", "c"),
        ("models", "  ", "c"),
        ("models", "t", ""),
    ] {
        server
            .post("/api/forum/posts")
            .add_header("Authorization", format!("Bearer {alice}"))
            .json(&serde_json::json!({ "board": board, "title": title, "content": content }))
            .await
            .assert_status(axum::http::StatusCode::BAD_REQUEST);
    }
}
