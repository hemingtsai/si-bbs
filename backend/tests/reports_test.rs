//! Reports: filing them, working the queue, and closing the loop when the content
//! is deleted directly.

mod common;

use axum::http::StatusCode;
use common::{register_and_login, register_login_as_role, test_ctx};

async fn create_forum_post(server: &axum_test::TestServer, token: &str, title: &str) -> i64 {
    server
        .post("/api/forum/posts")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({"board": "life", "title": title, "content": "body"}))
        .await
        .assert_status(StatusCode::CREATED)
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap()
}

async fn report(
    server: &axum_test::TestServer,
    token: &str,
    kind: &str,
    id: i64,
    reason: &str,
) -> axum_test::TestResponse {
    server
        .post("/api/reports")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({"target_kind": kind, "target_id": id, "reason": reason}))
        .await
}

#[tokio::test]
async fn reporting_posts_queues_them_for_staff_with_enough_context_to_triage() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    let post = create_forum_post(&server, &alice, "可疑内容").await;
    report(&server, &bob, "forum_post", post, "广告")
        .await
        .assert_status(StatusCode::CREATED);

    // Moderators see the queue (open by default) with the target title and reporter.
    let res = server
        .get("/api/reports")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 1);
    let item = &body["items"][0];
    assert_eq!(item["target_kind"], "forum_post");
    assert_eq!(item["target_id"], post);
    assert_eq!(item["reason"], "广告");
    assert_eq!(item["reporter_username"], "bob");
    assert_eq!(item["target_title"], "可疑内容");
    assert_eq!(item["target_deleted"], 0);
    assert_eq!(item["status"], "open");

    // The reporter can follow their own report.
    let mine = server
        .get("/api/reports/mine")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await;
    mine.assert_status_ok();
    assert_eq!(mine.json::<serde_json::Value>()["total"], 1);

    // A plain user has no access to the queue at all.
    server
        .get("/api/reports")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn reporting_the_same_thing_twice_does_not_queue_it_twice() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let post = create_forum_post(&server, &alice, "重复举报").await;

    report(&server, &bob, "forum_post", post, "第一次")
        .await
        .assert_status(StatusCode::CREATED);
    let second = report(&server, &bob, "forum_post", post, "第二次").await;
    // Idempotent rather than an error the caller has to understand.
    second.assert_status_ok();
    assert_eq!(
        second.json::<serde_json::Value>()["status"],
        "already reported"
    );

    let body = server
        .get("/api/reports")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(body["total"], 1);
}

#[tokio::test]
async fn only_live_public_content_can_be_reported() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;

    // Deleted post.
    let post = create_forum_post(&server, &alice, "将被删除").await;
    server
        .delete(&format!("/api/forum/posts/{post}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    report(&server, &bob, "forum_post", post, "x")
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // Draft wiki page: not public, so not reportable.
    let wiki = server
        .post("/api/wiki")
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"title":"草稿","category":"c","content":"x","status":"draft"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    report(&server, &bob, "wiki", wiki, "x")
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // Pending project.
    sqlx::query(
        "INSERT INTO projects (name, github_url, owner, repo, category, status, submitted_by) \
         VALUES ('待审', 'https://github.com/o/pending2', 'o', 'p2', 'c', 'pending', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    report(&server, &bob, "project", 1, "x")
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // Unknown id, and a nonsense kind.
    report(&server, &bob, "forum_post", 999_999, "x")
        .await
        .assert_status(StatusCode::NOT_FOUND);
    report(&server, &bob, "users", 1, "x")
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // An empty reason is rejected before any target lookup (cheap checks first).
    let live = create_forum_post(&server, &alice, "活着").await;
    report(&server, &bob, "forum_post", live, "   ")
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn reporting_requires_login() {
    let (server, _pool) = test_ctx().await;
    server
        .post("/api/reports")
        .json(&serde_json::json!({"target_kind":"forum_post","target_id":1,"reason":"x"}))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn resolving_a_report_records_the_decision_and_is_audited() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let post = create_forum_post(&server, &alice, "待处理").await;
    report(&server, &bob, "forum_post", post, "垃圾信息")
        .await
        .assert_status(StatusCode::CREATED);
    let report_id: i64 =
        sqlx::query_scalar("SELECT id FROM content_reports ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    // A plain user cannot resolve anything.
    server
        .patch(&format!("/api/reports/{report_id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .json(&serde_json::json!({"status":"dismissed"}))
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .patch(&format!("/api/reports/{report_id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({"status":"dismissed","note":"不是广告"}))
        .await
        .assert_status_ok();

    let (status, note, handled_by): (String, Option<String>, Option<i64>) =
        sqlx::query_as("SELECT status, note, handled_by FROM content_reports WHERE id = ?1")
            .bind(report_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "dismissed");
    assert_eq!(note.as_deref(), Some("不是广告"));
    assert!(handled_by.is_some());

    // Resolving twice is a conflict, not a silent second decision.
    server
        .patch(&format!("/api/reports/{report_id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({"status":"resolved"}))
        .await
        .assert_status(StatusCode::CONFLICT);

    // Bad status value.
    server
        .patch(&format!("/api/reports/{report_id}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({"status":"maybe"}))
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // And the decision is in the audit log.
    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(actions, ["report.resolve"]);
}

#[tokio::test]
async fn deleting_the_reported_content_closes_the_report() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;
    let post = create_forum_post(&server, &alice, "会被删掉").await;
    report(&server, &bob, "forum_post", post, "违规")
        .await
        .assert_status(StatusCode::CREATED);

    // A moderator removes the content instead of touching the report.
    server
        .delete(&format!("/api/forum/posts/{post}"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // The queue no longer holds it…
    let open = server
        .get("/api/reports")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(open["total"], 0, "a handled report stayed open: {open}");

    // …and it is closed with an explanation, not silently dropped.
    let resolved = server
        .get("/api/reports?status=resolved")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(resolved["total"], 1);
    assert_eq!(resolved["items"][0]["status"], "resolved");
    assert_eq!(resolved["items"][0]["note"], "content removed");
    assert_eq!(resolved["items"][0]["target_deleted"], 1);
}

#[tokio::test]
async fn one_account_cannot_flood_the_queue() {
    let (server, _pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;

    let limit = si_bbs_backend::services::ratelimit::REPORT_MAX_ATTEMPTS;
    for i in 0..limit {
        let post = create_forum_post(&server, &alice, &format!("帖子 {i}")).await;
        report(&server, &bob, "forum_post", post, "刷屏")
            .await
            .assert_status(StatusCode::CREATED);
    }
    let post = create_forum_post(&server, &alice, "再一个").await;
    let res = report(&server, &bob, "forum_post", post, "刷屏").await;
    assert_eq!(res.status_code(), StatusCode::TOO_MANY_REQUESTS);
    assert!(res.headers().get("retry-after").is_some());
}

#[tokio::test]
async fn the_queue_can_be_filtered_by_status_and_kind() {
    let (server, pool) = test_ctx().await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    let post = create_forum_post(&server, &alice, "帖子").await;
    let comment = server
        .post(&format!("/api/forum/posts/{post}/comments"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({"content":"回复内容"}))
        .await
        .json::<serde_json::Value>()["id"]
        .as_i64()
        .unwrap();
    report(&server, &bob, "forum_post", post, "帖子问题")
        .await
        .assert_status(StatusCode::CREATED);
    report(&server, &bob, "forum_comment", comment, "回复问题")
        .await
        .assert_status(StatusCode::CREATED);

    let by_kind = server
        .get("/api/reports?kind=forum_comment")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(by_kind["total"], 1);
    // The comment body is quoted so staff can judge it without another request.
    assert_eq!(by_kind["items"][0]["target_title"], "回复内容");

    server
        .get("/api/reports?kind=nonsense")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    server
        .get("/api/reports?status=maybe")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    let all = server
        .get("/api/reports?status=all")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await
        .json::<serde_json::Value>();
    assert_eq!(all["total"], 2);
}
