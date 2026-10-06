use mimalloc::MiMalloc;

use si_bbs_backend::{config::Config, db, routes::AppState};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// Say something *before* the first upload fails.
///
/// A directory that exists but is not writable by this process is the classic
/// container mistake (an image that chowns the parent but not the child), and the
/// only symptom otherwise is a 500 on the first upload — which may be days later.
/// This does not abort: content still works without attachments.
fn check_upload_dir(dir: &str) {
    let path = std::path::Path::new(dir);
    if let Err(err) = std::fs::create_dir_all(path) {
        tracing::error!(
            dir,
            error = %err,
            "upload directory is unusable; attachment uploads will fail with 500"
        );
        return;
    }
    let probe = path.join(".write-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(err) => tracing::error!(
            dir,
            error = %err,
            "upload directory is not writable by this process; attachment uploads will fail"
        ),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "si_bbs_backend=info,tower_http=info".into()),
        )
        .init();

    let cfg = Config::from_env();
    check_upload_dir(&cfg.upload_dir);
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
