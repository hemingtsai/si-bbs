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

    let app = si_bbs_backend::create_router(AppState { pool, cfg });
    // BIND_ADDR defaults to all interfaces for local Docker; on a host that
    // fronts with Caddy it should be 127.0.0.1:3000 so the API is not public.
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    tracing::info!("listening on {bind_addr}");
    axum::serve(listener, app).await.unwrap();
}
