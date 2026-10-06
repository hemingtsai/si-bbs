use mimalloc::MiMalloc;

use si_bbs_backend::{config::Config, db, routes::AppState};

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

    let cfg = Config::from_env();
    let pool = db::connect(&cfg.database_url)
        .await
        .expect("connect sqlite");
    db::migrate(&pool).await.expect("migrate");

    // Kept for the WAL checkpoint below: `AppState` takes its own handle.
    let shutdown_pool = pool.clone();
    let app = si_bbs_backend::create_router(AppState::new(pool, cfg));
    // BIND_ADDR defaults to all interfaces for local Docker; on a host that
    // fronts with Caddy it should be 127.0.0.1:3000 so the API is not public.
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    tracing::info!("listening on {bind_addr}");

    // `docker stop` sends SIGTERM; without this the process was killed mid-request
    // and the write-ahead log was left for the next start to recover. Draining
    // first means in-flight requests finish and the WAL can be folded back.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&shutdown_pool)
        .await
    {
        Ok(_) => tracing::info!("checkpointed sqlite wal, shutting down"),
        Err(err) => tracing::warn!(error = %err, "wal checkpoint failed"),
    }
}

/// Resolve when the process is asked to stop: Ctrl-C, or SIGTERM from an init
/// system or container runtime.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutdown signal received, draining connections");
}
