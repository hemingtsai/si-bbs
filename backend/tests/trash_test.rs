mod common;

use axum_test::TestServer;
use common::{register_and_login, register_login_as_role, test_ctx};
use sqlx::SqlitePool;

/// Each call needs a distinct GitHub URL because the column is UNIQUE even for
/// soft-deleted rows.
async fn seed_project(pool: &SqlitePool, owner: &str, status: &str, repo: &str) -> i64 {
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?1")
        .bind(owner)
        .fetch_one(pool)
        .await
        .unwrap();
    let res = sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, category, status, submitted_by) \
         VALUES (?3, ?4, 'o', ?3, 'dev-tools', ?2, ?1)",
    )
    .bind(user_id)
    .bind(status)
    .bind(repo)
    .bind(format!("https://github.com/o/{repo}"))
    .execute(pool)
    .await
    .unwrap();
    res.last_insert_rowid()
}

async fn seed_comment(pool: &SqlitePool, author: &str, project_id: i64, content: &str) -> i64 {
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?1")
        .bind(author)
        .fetch_one(pool)
        .await
        .unwrap();
    let res =
        sqlx::query("INSERT INTO comments (project_id, user_id, content) VALUES (?1, ?2, ?3)")
            .bind(project_id)
            .bind(user_id)
            .bind(content)
            .execute(pool)
            .await
            .unwrap();
    res.last_insert_rowid()
}

async fn trash_of(server: &TestServer, token: &str) -> Vec<serde_json::Value> {
    server
        .get("/api/trash")
        .add_header("Authorization", format!("Bearer {token}"))
        .await
        .assert_status_ok()
        .json::<Vec<serde_json::Value>>()
}

#[tokio::test]
async fn deleting_a_project_hides_it_and_lists_it_in_trash() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed").await;

    let res = server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status(axum::http::StatusCode::NO_CONTENT);

    // Gone from public endpoints...
    server
        .get(&format!("/api/projects/{project_id}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    let res = server.get("/api/projects").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 0);

    // ...but recoverable.
    let items = trash_of(&server, &alice).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "project");
    assert_eq!(items[0]["id"], project_id);
    assert_eq!(items[0]["name"], "doomed");
    assert_eq!(items[0]["deleted_by_username"], "alice");

    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM projects WHERE id = ?1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(deleted.is_some(), "row must survive for the trash phase");
}

#[tokio::test]
async fn project_delete_requires_owner_or_staff() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed").await;

    server
        .delete(&format!("/api/projects/{project_id}"))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    server
        .delete("/api/projects/999999")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Staff may delete another member's project.
    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // The deleter, not the original owner, owns the trash entry.
    let items = trash_of(&server, &mod_token).await;
    assert_eq!(items[0]["deleted_by_username"], "mod");
    let _ = alice;
}

#[tokio::test]
async fn deleting_a_comment_hides_it_from_the_list() {
    let (server, pool) = test_ctx().await;
    common::register(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed").await;
    let keep = seed_comment(&pool, "alice", project_id, "keep me").await;
    let drop = seed_comment(&pool, "bob", project_id, "remove me").await;

    let res = server
        .delete(&format!("/api/projects/{project_id}/comments/{drop}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await;
    res.assert_status(axum::http::StatusCode::NO_CONTENT);

    let res = server
        .get(&format!("/api/projects/{project_id}/comments"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["id"], keep);

    let items = trash_of(&server, &bob).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "comment");
    assert_eq!(items[0]["id"], drop);

    // Another member cannot delete it.
    server
        .delete(&format!("/api/projects/{project_id}/comments/{keep}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn comment_delete_checks_the_project_matches() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed-a").await;
    let other_project = seed_project(&pool, "alice", "approved", "doomed-b").await;
    let comment_id = seed_comment(&pool, "alice", project_id, "hello").await;

    server
        .delete(&format!(
            "/api/projects/{other_project}/comments/{comment_id}"
        ))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn trash_is_scoped_to_the_deleter_unless_staff() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    let a_project = seed_project(&pool, "alice", "approved", "doomed").await;
    let b_project = seed_project(&pool, "bob", "approved", "doomed-b").await;
    server
        .delete(&format!("/api/projects/{a_project}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/projects/{b_project}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    assert_eq!(trash_of(&server, &alice).await.len(), 1);
    assert_eq!(trash_of(&server, &bob).await.len(), 1);
    // Staff see the whole bin.
    assert_eq!(trash_of(&server, &mod_token).await.len(), 2);

    server
        .get("/api/trash")
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn restore_brings_items_back() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed").await;
    let comment_id = seed_comment(&pool, "alice", project_id, "restore me").await;

    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/projects/{project_id}/comments/{comment_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let res = server
        .post(&format!("/api/trash/project/{project_id}/restore"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["restored"], true);

    let res = server
        .post(&format!("/api/trash/comment/{comment_id}/restore"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status_ok();

    // Both are visible again.
    let res = server.get("/api/projects").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
    let res = server
        .get(&format!("/api/projects/{project_id}/comments"))
        .await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
    assert!(trash_of(&server, &alice).await.is_empty());
}

#[tokio::test]
async fn restore_wiki_page_returns_it_to_the_catalogue() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;

    let res = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({
            "title": "Guide", "category": "guides", "content": "body", "status": "published",
        }))
        .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    let slug = res.json::<serde_json::Value>()["slug"]
        .as_str()
        .unwrap()
        .to_string();

    server
        .delete(&format!("/api/wiki/page/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .get(&format!("/api/wiki/{slug}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    server
        .post(&format!("/api/trash/wiki/{id}/restore"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status_ok();

    server
        .get(&format!("/api/wiki/{slug}"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn restore_requires_the_deleter_or_staff() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed").await;

    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    server
        .post(&format!("/api/trash/project/{project_id}/restore"))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    server
        .post(&format!("/api/trash/project/{project_id}/restore"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    // Staff can restore on the author's behalf.
    server
        .post(&format!("/api/trash/project/{project_id}/restore"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn restore_rejects_unknown_kind_and_live_items() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed").await;

    server
        .post(&format!("/api/trash/user/{project_id}/restore"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    // Not deleted, so there is nothing to restore.
    server
        .post(&format!("/api/trash/project/{project_id}/restore"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_binned_project_still_reserves_its_github_url() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let project_id = seed_project(&pool, "alice", "approved", "reserved").await;

    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Soft deletion hides the row but does not release the unique key, so the
    // URL cannot be re-submitted and restoring can never collide.
    let taken: Result<i64, sqlx::Error> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE github_url = 'https://github.com/o/reserved'",
    )
    .fetch_one(&pool)
    .await;
    assert!(taken.is_ok(), "the binned row must still hold the URL");

    let status: String = sqlx::query_scalar("SELECT status FROM projects WHERE id = ?1")
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "approved");

    server
        .post(&format!("/api/trash/project/{project_id}/restore"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status_ok();
    let res = server.get("/api/projects").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
}

#[tokio::test]
async fn purge_is_admin_only_and_destroys_the_row() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let admin = register_login_as_role(&server, &pool, "root", "admin").await;
    let project_id = seed_project(&pool, "alice", "approved", "doomed").await;

    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    server
        .delete(&format!("/api/trash/project/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    // Moderators cannot purge permanently either.
    server
        .delete(&format!("/api/trash/project/{project_id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    server
        .delete(&format!("/api/trash/project/{project_id}"))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);

    server
        .delete(&format!("/api/trash/project/{project_id}"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?1")
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    // Purging something that is not in the bin is a 404.
    server
        .delete(&format!("/api/trash/project/{project_id}"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

/// Guard against the trash view drifting away from the handler's whitelist.
///
/// The old version iterated a view that had no rows at all, so its loop body
/// never ran. It now seeds every kind and compares the two lists in both
/// directions: a kind the view can emit but the handler does not know about
/// cannot be restored or purged, and a kind the handler lists but the view never
/// emits answers 404 forever.
#[tokio::test]
async fn trash_view_only_contains_known_kinds() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let wiki = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"title":"t","category":"c","content":"x"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .delete(&format!("/api/wiki/page/{wiki}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let project = seed_project(&pool, "alice", "approved", "kinds").await;
    server
        .delete(&format!("/api/projects/{project}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let comment = seed_comment(&pool, "alice", project, "hello").await;
    server
        .delete(&format!("/api/projects/{project}/comments/{comment}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let post = server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"board":"life","title":"p","content":"x"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    let reply = server
        .post(&format!("/api/forum/posts/{post}/comments"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"content":"r"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .delete(&format!("/api/forum/comments/{reply}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/forum/posts/{post}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let mut view_kinds: Vec<String> = sqlx::query_scalar("SELECT DISTINCT kind FROM trash_view")
        .fetch_all(&pool)
        .await
        .unwrap();
    view_kinds.sort();
    let mut handler_kinds: Vec<String> = si_bbs_backend::handlers::trash::KINDS
        .iter()
        .map(|k| k.to_string())
        .collect();
    handler_kinds.sort();

    assert_eq!(view_kinds, handler_kinds);
}

/// The binary must keep building against the same router the tests exercise, and
/// that router must actually serve the trash endpoints. The previous version only
/// constructed a router and dropped it, so deleting every trash route still passed.
#[tokio::test]
async fn trash_routes_are_mounted_in_the_shared_router() {
    let (server, _pool) = test_ctx().await;

    // 401 rather than 404: the route exists and asks for a token.
    for (method, path) in [
        ("GET", "/api/trash".to_string()),
        ("POST", "/api/trash/wiki/1/restore".to_string()),
        ("DELETE", "/api/trash/wiki/1".to_string()),
        // An unknown kind is only distinguishable from an unmounted route once
        // the caller is authenticated; the integration tests cover that.
    ] {
        let res = match method {
            "GET" => server.get(&path).await,
            "POST" => server.post(&path).await,
            _ => server.delete(&path).await,
        };
        assert_eq!(
            res.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "{method} {path} is not mounted"
        );
    }
}

/// Regression: `comments.project_id` and `ratings.project_id` have no
/// `ON DELETE` rule, so purging a project that was commented on or rated used to
/// fail with `FOREIGN KEY constraint failed` and surface as a 500.
#[tokio::test]
async fn purging_a_project_removes_its_comments_and_ratings() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let admin = register_login_as_role(&server, &pool, "root", "admin").await;
    let project_id = seed_project(&pool, "alice", "approved", "purge-me").await;

    let bob = register_and_login(&server, "bob").await;
    seed_comment(&pool, "bob", project_id, "first").await;
    server
        .post(&format!("/api/projects/{project_id}/rating"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({ "score": 8, "comment": "solid" }))
        .await
        .assert_status_ok();

    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    server
        .delete(&format!("/api/trash/project/{project_id}"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?1")
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let comments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE project_id = ?1")
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let ratings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ratings WHERE project_id = ?1")
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((projects, comments, ratings), (0, 0, 0));
}

/// Regression: same foreign-key trap one level down — a post with replies (and
/// likes on both) could not be purged.
#[tokio::test]
async fn purging_a_forum_post_removes_replies_and_likes() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let admin = register_login_as_role(&server, &pool, "root", "admin").await;
    let bob = register_and_login(&server, "bob").await;

    let post = server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"board":"life","title":"doomed","content":"x"}))
        .await
        .json::<serde_json::Value>();
    let post_id = post["id"].as_i64().unwrap();

    let reply = server
        .post(&format!("/api/forum/posts/{post_id}/comments"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({"content":"reply"}))
        .await
        .json::<serde_json::Value>();
    let reply_id = reply["id"].as_i64().unwrap();

    // Likes on the post and on the reply must both be cleaned up.
    for (path, token) in [
        (format!("/api/forum/posts/{post_id}/like"), &alice),
        (format!("/api/forum/comments/{reply_id}/like"), &bob),
    ] {
        server
            .post(&path)
            .add_header("Authorization", format!("Bearer {token}"))
            .await
            .assert_status_ok();
    }

    server
        .delete(&format!("/api/forum/posts/{post_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    server
        .delete(&format!("/api/trash/forum_post/{post_id}"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let posts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM forum_posts WHERE id = ?1")
        .bind(post_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let replies: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM forum_comments WHERE post_id = ?1")
        .bind(post_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let likes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM forum_likes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((posts, replies, likes), (0, 0, 0));
}

/// Purging a single reply must not touch its post, and must drop that reply's
/// likes only.
#[tokio::test]
async fn purging_a_forum_comment_keeps_the_post_and_drops_its_likes() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let admin = register_login_as_role(&server, &pool, "root", "admin").await;
    let bob = register_and_login(&server, "bob").await;

    let post_id = server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"board":"life","title":"survivor","content":"x"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();

    let mut replies = Vec::new();
    for content in ["keep me", "delete me"] {
        let id = server
            .post(&format!("/api/forum/posts/{post_id}/comments"))
            .add_header("Authorization", format!("Bearer {bob}"))
            .json(&serde_json::json!({ "content": content }))
            .await
            .json::<serde_json::Value>()["id"]
            .as_i64()
            .unwrap();
        replies.push(id);
    }
    let doomed = replies[1];

    server
        .post(&format!("/api/forum/comments/{doomed}/like"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status_ok();

    server
        .delete(&format!("/api/forum/comments/{doomed}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/trash/forum_comment/{doomed}"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let reply: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM forum_comments WHERE id = ?1")
        .bind(doomed)
        .fetch_one(&pool)
        .await
        .unwrap();
    let kept: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM forum_comments WHERE id = ?1")
        .bind(replies[0])
        .fetch_one(&pool)
        .await
        .unwrap();
    let post: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM forum_posts WHERE id = ?1")
        .bind(post_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let likes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM forum_likes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((reply, kept, post, likes), (0, 1, 1, 0));
}

/// Purging is recorded in the same transaction as the delete: the log can never
/// claim a row was destroyed when the delete rolled back.
#[tokio::test]
async fn purging_is_recorded_in_the_audit_log() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let admin = register_login_as_role(&server, &pool, "root", "admin").await;
    let project_id = seed_project(&pool, "alice", "approved", "audited").await;

    server
        .delete(&format!("/api/projects/{project_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/trash/project/{project_id}"))
        .add_header("Authorization", format!("Bearer {admin}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    let (action, kind, target): (String, String, i64) = sqlx::query_as(
        "SELECT action, target_kind, target_id FROM audit_log ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (action.as_str(), kind.as_str(), target),
        ("trash.purge", "project", project_id)
    );

    // A restore that a moderator performs on someone else's item is recorded too.
    let wiki_id = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"title":"t","category":"c","content":"x"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    server
        .delete(&format!("/api/wiki/page/{wiki_id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    server
        .post(&format!("/api/trash/wiki/{wiki_id}/restore"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status_ok();

    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(actions, ["trash.purge", "trash.restore"]);
}
