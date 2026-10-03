use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "si_bbs_backend=info,tower_http=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://si-bbs.db?mode=rwc".to_string());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect sqlite");
    sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await.ok();
    sqlx::query("PRAGMA synchronous=NORMAL").execute(&pool).await.ok();
    sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await.ok();

    let app = axum::Router::new()
        .route("/api/health", axum::routing::get(|| async { "ok" }))
        .layer(tower::limit::ConcurrencyLimitLayer::new(64));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("listening on 0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
