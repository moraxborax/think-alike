mod app;
mod auth;
mod clustering;
mod config;
mod db;
mod embedding;
mod error;
mod routes;
mod static_files;

use anyhow::Context;
use app::build_router;
use config::Config;
use db::build_pool;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Arc::new(Config::from_env()?);
    let pool = build_pool(&config.database_url).await?;
    db::run_migrations(&pool).await?;

    let app = build_router(config, pool).await;
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    info!("listening on {addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("server failed")?;
    Ok(())
}
