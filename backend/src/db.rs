use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::str::FromStr;
use std::time::Duration;

/// Connections sqlx may open. SQLite serialises writers anyway, so this is about
/// overlapping readers plus the occasional writer on a small box.
const MAX_CONNECTIONS: u32 = 5;
/// How long a writer waits for the lock before giving up with `SQLITE_BUSY`.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    // These settings are **per connection** in SQLite, so they are declared as connect
    // options: every connection the pool opens gets them.
    //
    // They used to be applied with `sqlx::query("PRAGMA ...").execute(&pool)`, which
    // checks out exactly one connection of five. Measured on this build, sqlx's own
    // defaults already cover `foreign_keys` (on), `busy_timeout` (5s) and
    // `journal_mode` (WAL for a file), so the value that actually drifted was
    // `synchronous`: one connection ran NORMAL and the other four ran FULL, i.e. most
    // commits fsync'd and some did not. On a single-core box that is both wasted I/O
    // and an inconsistent durability guarantee. Declaring all four here means the
    // behaviour no longer depends on a library default that could change under us.
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        // NORMAL is the documented pairing for WAL: it survives a process crash and
        // only risks the last commits on a power loss, which is the right trade for
        // this workload.
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        // WAL allows one writer at a time. Without a timeout the second writer fails
        // immediately instead of waiting the few milliseconds the first one needs.
        .busy_timeout(BUSY_TIMEOUT);

    SqlitePoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .acquire_timeout(BUSY_TIMEOUT)
        .connect_with(options)
        .await
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
