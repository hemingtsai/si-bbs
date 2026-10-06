//! Connection-pool configuration.
//!
//! `foreign_keys`, `synchronous` and `busy_timeout` are **per connection** in SQLite,
//! so a `PRAGMA` statement run against the pool configures exactly one connection.
//! These used to be applied that way: on this build sqlx already defaults
//! `foreign_keys` on, `busy_timeout` to 5s and the journal mode to WAL, so the setting
//! that really drifted was `synchronous` — one connection NORMAL, four FULL.
//!
//! The tests below pin the *outcome* (every connection agrees, and cascades really
//! happen) rather than the mechanism, so they keep holding if a default changes.

use std::time::Duration;

/// A file-backed database, because an in-memory one gives every connection its own
/// private database and the pool would never open a second connection.
struct TempDb {
    path: std::path::PathBuf,
}

impl TempDb {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("si-bbs-pool-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Self { path }
    }

    fn url(&self) -> String {
        format!("sqlite://{}?mode=rwc", self.path.display())
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
        }
    }
}

#[tokio::test]
async fn every_connection_in_the_pool_gets_the_sqlite_settings() {
    let db = TempDb::new("pragmas");
    let pool = si_bbs_backend::db::connect(&db.url())
        .await
        .expect("connect");

    // Hold a connection per slot: the pool has to open all of them at once, so each
    // assertion below runs on a *different* connection.
    let slots = pool.options().get_max_connections() as usize;
    assert!(slots > 1, "this test is pointless with a single connection");
    let mut held = Vec::new();
    for _ in 0..slots {
        held.push(pool.begin().await.expect("checkout"));
    }

    for (index, tx) in held.iter_mut().enumerate() {
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **tx)
            .await
            .unwrap();
        assert_eq!(
            foreign_keys, 1,
            "connection {index} has foreign keys OFF: cascade deletes would leave orphans"
        );

        let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&mut **tx)
            .await
            .unwrap();
        assert!(
            busy_timeout > 0,
            "connection {index} has no busy timeout: concurrent writes fail with SQLITE_BUSY"
        );

        let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(&mut **tx)
            .await
            .unwrap();
        // 1 = NORMAL, the recommended setting for WAL.
        assert_eq!(
            synchronous, 1,
            "connection {index} is not synchronous=NORMAL"
        );
    }

    // WAL is a property of the database file, so it is enough to see it once.
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut *held[0])
        .await
        .unwrap();
    assert_eq!(journal.to_lowercase(), "wal");
}

/// The behaviour the settings exist for. Foreign keys are enforced on every pooled
/// connection — a guard rather than a fix, since this build's default already is ON,
/// but the cascade is exactly what the bin's purge path relies on to delete a parent
/// with children.
#[tokio::test]
async fn foreign_keys_are_actually_enforced_on_a_pooled_connection() {
    let db = TempDb::new("fk");
    let pool = si_bbs_backend::db::connect(&db.url())
        .await
        .expect("connect");

    sqlx::query("CREATE TABLE parent (id INTEGER PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL \
         REFERENCES parent(id) ON DELETE CASCADE)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO parent (id) VALUES (1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO child (id, parent_id) VALUES (1, 1)")
        .execute(&pool)
        .await
        .unwrap();

    // Sequential requests all reuse one connection, so the pool has to be *forced* to
    // open the others: hold every slot but the last, then run the check on the
    // connection that never saw the configuration.
    let slots = pool.options().get_max_connections() as usize;
    let mut held = Vec::new();
    for _ in 0..slots - 1 {
        held.push(pool.begin().await.expect("checkout"));
    }
    let rejected = sqlx::query("INSERT INTO child (id, parent_id) VALUES (99, 12345)")
        .execute(&pool)
        .await;
    assert!(
        rejected.is_err(),
        "an orphan child row was accepted on a pooled connection: foreign keys are not \
         enforced everywhere"
    );
    // And again on the connections that were just released.
    drop(held);
    for _ in 0..12 {
        let rejected = sqlx::query("INSERT INTO child (id, parent_id) VALUES (99, 12345)")
            .execute(&pool)
            .await;
        assert!(rejected.is_err(), "an orphan child row was accepted");
    }

    // …and the cascade itself happens.
    sqlx::query("DELETE FROM parent WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    let orphans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM child")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(orphans, 0, "the cascade did not fire");

    // A second writer waits for the lock instead of failing instantly with
    // SQLITE_BUSY — which is what the busy timeout buys.
    let mut first = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO parent (id) VALUES (2)")
        .execute(&mut *first)
        .await
        .unwrap();

    let waiter_pool = pool.clone();
    let waiter = tokio::spawn(async move {
        sqlx::query("INSERT INTO parent (id) VALUES (3)")
            .execute(&waiter_pool)
            .await
    });
    // Long enough that a "fail immediately" implementation would have finished.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !waiter.is_finished(),
        "the second writer gave up instead of waiting for the lock"
    );

    first.commit().await.unwrap();
    let inserted = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("the waiter should finish once the lock is released")
        .expect("join");
    assert!(
        inserted.is_ok(),
        "the waited-for write did not succeed: {inserted:?}"
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM parent")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 2);
}
