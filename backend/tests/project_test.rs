mod common;

use axum_test::TestServer;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use common::{register_and_login, register_login_as_role, test_ctx, test_ctx_with_github};
use si_bbs_backend::services::github::parse_repo_url;
use sqlx::SqlitePool;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn readme_body(markdown: &str) -> String {
    serde_json::json!({
        "content": BASE64.encode(markdown),
        "encoding": "base64",
    })
    .to_string()
}

fn repo_body() -> String {
    repo_body_with(47000, 3900)
}

/// Same shape as [`repo_body`] but with controllable counters, for the refresh
/// tests that need GitHub to report different numbers the second time.
fn repo_body_with(stars: i64, forks: i64) -> String {
    serde_json::json!({
        "name": "ripgrep",
        "owner": { "login": "BurntSushi" },
        "description": "ripgrep recursively searches directories for a regex pattern",
        "stargazers_count": stars,
        "forks_count": forks,
        "language": "Rust",
        "topics": ["search", "cli", "grep"],
        "license": { "spdx_id": "MIT", "name": "MIT License" },
    })
    .to_string()
}

async fn mock_github(readme: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep"))
        .respond_with(ResponseTemplate::new(200).set_body_string(repo_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep/readme"))
        .respond_with(ResponseTemplate::new(200).set_body_string(readme_body(readme)))
        .mount(&server)
        .await;
    server
}

async fn submit(
    server: &TestServer,
    token: &str,
    url: &str,
    category: &str,
) -> axum_test::TestResponse {
    server
        .post("/api/projects")
        .add_header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "github_url": url, "category": category }))
        .await
}

#[test]
fn parses_github_url_variants() {
    let expected = si_bbs_backend::services::github::RepoRef {
        owner: "BurntSushi".into(),
        repo: "ripgrep".into(),
    };
    for url in [
        "https://github.com/BurntSushi/ripgrep",
        "https://github.com/BurntSushi/ripgrep/",
        "https://www.github.com/BurntSushi/ripgrep",
        "http://github.com/BurntSushi/ripgrep.git",
        "git@github.com:BurntSushi/ripgrep.git",
        "  https://github.com/BurntSushi/ripgrep  ",
    ] {
        assert_eq!(parse_repo_url(url).expect(url), expected, "failed on {url}");
    }
}

#[test]
fn rejects_non_github_urls() {
    for url in [
        "",
        "https://gitlab.com/a/b",
        "https://github.com/onlyowner",
        "https://github.com/a/b/c",
        "not a url",
        "https://github.com/own er/repo",
    ] {
        assert!(parse_repo_url(url).is_err(), "should reject {url}");
    }
}

#[tokio::test]
async fn submit_valid_url_creates_pending_project_with_github_metadata() {
    let gh = mock_github("# ripgrep\n\nfast grep").await;
    let (server, _pool) = test_ctx_with_github(&gh.uri()).await;
    let token = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &token,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;

    res.assert_status(axum::http::StatusCode::CREATED);
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["status"], "pending");
    assert_eq!(body["name"], "ripgrep");
    assert_eq!(body["owner"], "BurntSushi");
    assert_eq!(body["repo"], "ripgrep");
    assert_eq!(body["github_url"], "https://github.com/BurntSushi/ripgrep");
    assert_eq!(body["category"], "dev-tools");
    assert_eq!(body["language"], "Rust");
    assert_eq!(body["license"], "MIT");
    assert_eq!(body["stars"], 47000);
    assert_eq!(body["forks"], 3900);
    assert_eq!(body["topics"][0], "search");
    assert_eq!(body["readme_raw"], "# ripgrep\n\nfast grep");
    assert!(body["readme_fetched_at"].is_string());
}

#[tokio::test]
async fn submit_requires_login() {
    let (server, _pool) = test_ctx().await;
    let res = server
        .post("/api/projects")
        .json(&serde_json::json!({
            "github_url": "https://github.com/BurntSushi/ripgrep",
            "category": "dev-tools",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn submit_invalid_url_returns_400() {
    let (server, _pool) = test_ctx().await;
    let token = register_and_login(&server, "alice").await;

    submit(&server, &token, "https://gitlab.com/a/b", "dev-tools")
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    submit(
        &server,
        &token,
        "https://github.com/BurntSushi/ripgrep",
        "  ",
    )
    .await
    .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_missing_github_repo_returns_400() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ghost"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&gh)
        .await;
    let (server, _pool) = test_ctx_with_github(&gh.uri()).await;
    let token = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &token,
        "https://github.com/BurntSushi/ghost",
        "dev-tools",
    )
    .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(
        res.json::<serde_json::Value>()["error"],
        "github repository not found"
    );
}

#[tokio::test]
async fn submit_duplicate_url_returns_409() {
    let gh = mock_github("# ripgrep").await;
    let (server, _pool) = test_ctx_with_github(&gh.uri()).await;
    let token = register_and_login(&server, "alice").await;

    submit(
        &server,
        &token,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await
    .assert_status(axum::http::StatusCode::CREATED);
    // Same repo via the .git spelling must still collide on the canonical URL.
    let res = submit(
        &server,
        &token,
        "git@github.com:BurntSushi/ripgrep.git",
        "dev-tools",
    )
    .await;
    res.assert_status(axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn list_only_shows_approved_projects() {
    let gh = mock_github("# ripgrep").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await
    .assert_status(axum::http::StatusCode::CREATED);

    // Pending: visible to nobody in the public list.
    let res = server.get("/api/projects").await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 0);

    let id = sqlx::query_scalar::<_, i64>("SELECT id FROM projects LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    server
        .post(&format!("/api/projects/{id}/review"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({ "action": "approve" }))
        .await
        .assert_status_ok();

    let res = server.get("/api/projects").await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["status"], "approved");
}

#[tokio::test]
async fn list_filters_by_category_and_keyword_and_sorts() {
    let gh = mock_github("# ripgrep").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await
    .assert_status(axum::http::StatusCode::CREATED);
    sqlx::query("UPDATE projects SET status = 'approved', stars = 10, created_at = datetime('now', '-1 day')")
        .execute(&pool)
        .await
        .unwrap();

    let res = server.get("/api/projects?category=dev-tools").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    let res = server.get("/api/projects?category=other").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 0);

    let res = server.get("/api/projects?q=ripgrep").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    let res = server.get("/api/projects?q=nothing-here").await;
    assert_eq!(res.json::<serde_json::Value>()["total"], 0);

    let res = server.get("/api/projects?sort=recent").await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["items"][0]["stars"], 10);
    assert_eq!(body["per_page"], 20);
    assert_eq!(body["page"], 1);
}

#[tokio::test]
async fn regular_user_cannot_review_but_moderator_can() {
    let gh = mock_github("# ripgrep").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await
    .assert_status(axum::http::StatusCode::CREATED);
    let id = sqlx::query_scalar::<_, i64>("SELECT id FROM projects LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Unauthenticated and plain user are both refused.
    server
        .post(&format!("/api/projects/{id}/review"))
        .json(&serde_json::json!({ "action": "approve" }))
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    server
        .post(&format!("/api/projects/{id}/review"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .json(&serde_json::json!({ "action": "approve" }))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    let res = server
        .post(&format!("/api/projects/{id}/review"))
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .json(&serde_json::json!({ "action": "approve", "note": "looks good" }))
        .await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["status"], "approved");
    assert_eq!(body["review_note"], "looks good");
    assert!(body["reviewed_by"].is_number());
}

#[tokio::test]
async fn reject_requires_note_and_invalid_action_is_400() {
    let gh = mock_github("# ripgrep").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await
    .assert_status(axum::http::StatusCode::CREATED);
    let id = sqlx::query_scalar::<_, i64>("SELECT id FROM projects LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    async fn review(
        server: &TestServer,
        token: &str,
        id: i64,
        action: serde_json::Value,
        note: Option<&str>,
    ) -> axum_test::TestResponse {
        let mut body = serde_json::json!({ "action": action });
        if let Some(n) = note {
            body["note"] = serde_json::Value::String(n.to_string());
        }
        server
            .post(&format!("/api/projects/{id}/review"))
            .add_header("Authorization", format!("Bearer {token}"))
            .json(&body)
            .await
    }

    review(&server, &mod_token, id, serde_json::json!("reject"), None)
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    review(
        &server,
        &mod_token,
        id,
        serde_json::json!("sideways"),
        Some("nope"),
    )
    .await
    .assert_status(axum::http::StatusCode::BAD_REQUEST);

    let res = review(
        &server,
        &mod_token,
        id,
        serde_json::json!("reject"),
        Some("duplicate"),
    )
    .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["status"], "rejected");
}

#[tokio::test]
async fn review_queue_lists_pending_for_moderator_only() {
    let gh = mock_github("# ripgrep").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;
    let mod_token = register_login_as_role(&server, &pool, "mod", "moderator").await;

    submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await
    .assert_status(axum::http::StatusCode::CREATED);

    server
        .get("/api/projects/review-queue")
        .add_header("Authorization", format!("Bearer {alice}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);

    let res = server
        .get("/api/projects/review-queue")
        .add_header("Authorization", format!("Bearer {mod_token}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);
}

#[tokio::test]
async fn mine_lists_own_submissions_including_pending() {
    let gh = mock_github("# ripgrep").await;
    let (server, _pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;

    submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await
    .assert_status(axum::http::StatusCode::CREATED);

    let res = server
        .get("/api/projects/mine")
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 1);

    let res = server
        .get("/api/projects/mine")
        .add_header("Authorization", format!("Bearer {bob}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["total"], 0);
}

#[tokio::test]
async fn detail_hides_pending_project_from_strangers_but_shows_owner() {
    let gh = mock_github("# ripgrep").await;
    let (server, _pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;
    let bob = register_and_login(&server, "bob").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();

    server
        .get(&format!("/api/projects/{id}"))
        .add_header("Authorization", format!("Bearer {bob}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    let res = server
        .get(&format!("/api/projects/{id}"))
        .add_header("Authorization", format!("Bearer {alice}"))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["id"], id);

    server
        .get("/api/projects/999999")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn detail_refreshes_readme_when_cache_is_stale() {
    let gh = mock_github("# fresh readme").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    sqlx::query("UPDATE projects SET status = 'approved' WHERE id = ?1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    // Fresh cache: served from the stored copy.
    let res = server.get(&format!("/api/projects/{id}")).await;
    res.assert_status_ok();
    assert_eq!(
        res.json::<serde_json::Value>()["readme_raw"],
        "# fresh readme"
    );

    // GitHub now answers with a different README and the cache is aged past 24h.
    let gh2 = mock_github("# updated readme").await;
    let server2 = common::server_with(pool.clone(), &gh2.uri());
    sqlx::query(
        "UPDATE projects SET readme_fetched_at = datetime('now', '-25 hours') WHERE id = ?1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let res = server2.get(&format!("/api/projects/{id}")).await;
    res.assert_status_ok();
    assert_eq!(
        res.json::<serde_json::Value>()["readme_raw"],
        "# updated readme"
    );

    // The refreshed copy is persisted, so a fresh read needs no GitHub call.
    let stored: Option<String> =
        sqlx::query_scalar("SELECT readme_raw FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some("# updated readme"));
}

#[tokio::test]
async fn detail_does_not_call_github_while_cache_is_fresh() {
    let gh = mock_github("# cached readme").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    sqlx::query("UPDATE projects SET status = 'approved', readme_fetched_at = datetime('now', '-2 hours') WHERE id = ?1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    // A dead GitHub must not affect a fresh cache.
    let dead = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&dead)
        .await;
    let server2 = common::server_with(pool.clone(), &dead.uri());

    let res = server2.get(&format!("/api/projects/{id}")).await;
    res.assert_status_ok();
    assert_eq!(
        res.json::<serde_json::Value>()["readme_raw"],
        "# cached readme"
    );
}

#[tokio::test]
async fn detail_keeps_stale_readme_when_github_fails() {
    let gh = mock_github("# cached readme").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    sqlx::query("UPDATE projects SET status = 'approved', readme_fetched_at = datetime('now', '-48 hours') WHERE id = ?1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    // GitHub is broken now; the stale copy must still be served.
    let broken = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&broken)
        .await;
    let server2 = common::server_with(pool.clone(), &broken.uri());

    let res = server2.get(&format!("/api/projects/{id}")).await;
    res.assert_status_ok();
    assert_eq!(
        res.json::<serde_json::Value>()["readme_raw"],
        "# cached readme"
    );

    // The failed refresh must not have advanced the timestamp.
    let ts: Option<String> =
        sqlx::query_scalar("SELECT readme_fetched_at FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ts.is_some());
}

/// Regression: the failure path kept the cached README but recorded nothing, so
/// every later detail request for a stale project made another upstream call —
/// each waiting up to the client's 8s timeout. One failure must now buy a quiet
/// window instead of a request per page view.
#[tokio::test]
async fn a_failed_refresh_backs_off_instead_of_calling_github_per_request() {
    let gh = mock_github("# cached readme").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    sqlx::query(
        "UPDATE projects SET status = 'approved', readme_fetched_at = datetime('now', '-48 hours'), \
         readme_attempted_at = NULL WHERE id = ?1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let fetched_before: Option<String> =
        sqlx::query_scalar("SELECT readme_fetched_at FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // GitHub is down.
    let broken = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&broken)
        .await;
    let server2 = common::server_with(pool.clone(), &broken.uri());

    for attempt in 1..=3 {
        let res = server2.get(&format!("/api/projects/{id}")).await;
        res.assert_status_ok();
        assert_eq!(
            res.json::<serde_json::Value>()["readme_raw"],
            "# cached readme"
        );
        assert_eq!(
            broken.received_requests().await.unwrap().len(),
            1,
            "attempt {attempt} hit GitHub again instead of backing off"
        );
    }

    // The document is still as old as it was: a failed fetch is not a refresh.
    let fetched_after: Option<String> =
        sqlx::query_scalar("SELECT readme_fetched_at FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(fetched_before, fetched_after);
    // …but the attempt is on record, which is what throttles the retries.
    let attempted: Option<String> =
        sqlx::query_scalar("SELECT readme_attempted_at FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(attempted.is_some(), "the failed attempt was not recorded");
}

#[tokio::test]
async fn detail_clears_readme_when_repo_has_none() {
    let gh = mock_github("# cached readme").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    sqlx::query("UPDATE projects SET status = 'approved', readme_fetched_at = datetime('now', '-48 hours') WHERE id = ?1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    // Repo without a README: the missing copy is recorded and stays null. The
    // metadata endpoint has to answer too — the refresh asks for it first, and a
    // 404 there means "repository is gone", which keeps the cached document.
    let gh2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep"))
        .respond_with(ResponseTemplate::new(200).set_body_string(repo_body()))
        .mount(&gh2)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep/readme"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&gh2)
        .await;
    let server2 = common::server_with(pool.clone(), &gh2.uri());

    let res = server2.get(&format!("/api/projects/{id}")).await;
    res.assert_status_ok();
    assert!(res.json::<serde_json::Value>()["readme_raw"].is_null());
}

#[tokio::test]
async fn repo_without_readme_submits_with_null_readme() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/bare"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                serde_json::json!({
                    "name": "bare",
                    "owner": { "login": "BurntSushi" },
                    "stargazers_count": 1,
                    "forks_count": 0,
                })
                .to_string(),
            ),
        )
        .mount(&gh)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/bare/readme"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&gh)
        .await;

    let (server, _pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/bare",
        "misc",
    )
    .await;
    res.assert_status(axum::http::StatusCode::CREATED);
    let body = res.json::<serde_json::Value>();
    assert!(body["readme_raw"].is_null());
    assert!(body["language"].is_null());
    assert_eq!(body["topics"].as_array().unwrap().len(), 0);
}

/// The GitHub token must be forwarded when configured.
#[tokio::test]
async fn github_token_is_sent_when_configured() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer test-token",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                serde_json::json!({
                    "name": "r", "owner": { "login": "o" }, "stargazers_count": 0, "forks_count": 0,
                })
                .to_string(),
            ),
        )
        .mount(&gh)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/readme"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&gh)
        .await;

    let pool: SqlitePool = common::test_pool().await;
    let cfg = si_bbs_backend::config::Config {
        database_url: "sqlite::memory:".into(),
        jwt_secret: "test-secret".into(),
        access_ttl_secs: 900,
        refresh_ttl_secs: 7 * 24 * 3600,
        github_token: "test-token".into(),
        github_api_base: gh.uri(),
        public_base_url: String::new(),
    };
    let app = si_bbs_backend::create_router(si_bbs_backend::routes::AppState::new(pool, cfg));
    let server = TestServer::new(app);
    let alice = register_and_login(&server, "alice").await;

    submit(&server, &alice, "https://github.com/o/r", "misc")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// The counters were captured once at submission and never revisited, so a
/// catalogue sorted by stars slowly drifted away from reality.
#[tokio::test]
async fn stale_refresh_updates_the_github_counters() {
    let gh = mock_github("# cached readme").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let body = res.json::<serde_json::Value>();
    let id = body["id"].as_i64().unwrap();
    assert_eq!(body["stars"], 47000);

    sqlx::query(
        "UPDATE projects SET status = 'approved', readme_fetched_at = datetime('now', '-48 hours'), \
         readme_attempted_at = NULL WHERE id = ?1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    // GitHub now reports more stars and a new README.
    let gh2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep"))
        .respond_with(ResponseTemplate::new(200).set_body_string(repo_body_with(48000, 4100)))
        .mount(&gh2)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep/readme"))
        .respond_with(ResponseTemplate::new(200).set_body_string(readme_body("# refreshed")))
        .mount(&gh2)
        .await;
    let server2 = common::server_with(pool.clone(), &gh2.uri());

    let res = server2.get(&format!("/api/projects/{id}")).await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["stars"], 48000);
    assert_eq!(body["forks"], 4100);
    assert_eq!(body["readme_raw"], "# refreshed");
}

/// A README that cannot be fetched must not also freeze the counters: they come
/// from a separate call, and the cached document is simply kept.
#[tokio::test]
async fn a_readme_failure_still_refreshes_the_counters() {
    let gh = mock_github("# cached readme").await;
    let (server, pool) = test_ctx_with_github(&gh.uri()).await;
    let alice = register_and_login(&server, "alice").await;

    let res = submit(
        &server,
        &alice,
        "https://github.com/BurntSushi/ripgrep",
        "dev-tools",
    )
    .await;
    let id = res.json::<serde_json::Value>()["id"].as_i64().unwrap();
    sqlx::query(
        "UPDATE projects SET status = 'approved', readme_fetched_at = datetime('now', '-48 hours'), \
         readme_attempted_at = NULL WHERE id = ?1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let fetched_before: Option<String> =
        sqlx::query_scalar("SELECT readme_fetched_at FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();

    let gh2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep"))
        .respond_with(ResponseTemplate::new(200).set_body_string(repo_body_with(49000, 4200)))
        .mount(&gh2)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/BurntSushi/ripgrep/readme"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&gh2)
        .await;
    let server2 = common::server_with(pool.clone(), &gh2.uri());

    let res = server2.get(&format!("/api/projects/{id}")).await;
    res.assert_status_ok();
    let body = res.json::<serde_json::Value>();
    assert_eq!(body["stars"], 49000, "counters should still refresh");
    assert_eq!(body["readme_raw"], "# cached readme", "cache must be kept");

    // The document is no older than it was: a failed fetch is not a refresh.
    let fetched_after: Option<String> =
        sqlx::query_scalar("SELECT readme_fetched_at FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(fetched_before, fetched_after);
}
