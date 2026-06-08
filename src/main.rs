#![allow(
    clippy::collapsible_if,
    clippy::redundant_closure,
    clippy::redundant_pattern_matching,
    clippy::needless_borrows_for_generic_args,
    clippy::io_other_error,
    clippy::if_same_then_else,
    clippy::manual_pattern_char_comparison,
    clippy::derivable_impls,
    clippy::items_after_test_module,
    clippy::overly_complex_bool_expr,
    clippy::too_many_arguments,
    clippy::new_without_default,
    clippy::needless_bool,
    clippy::collapsible_else_if
)]

mod api;
mod app;
mod auth;
mod cli;
mod config;
mod db;
mod embedded;
mod error;
mod iam;
mod metrics;
mod server;
mod stats;
mod storage;
mod xml;

use std::net::SocketAddr;
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let config = match cli::execute(cli).await? {
        Some(config) => config,
        None => return Ok(()),
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    if config.access_key == "maxioadmin"
        && config.secret_key == "maxioadmin"
        && !config.allow_insecure_dev
    {
        anyhow::bail!(
            "refusing to start with default credentials in production; set MAXIO_ACCESS_KEY/MAXIO_SECRET_KEY or use --allow-insecure-dev for local development"
        );
    }

    let state = app::build_app_state(config.clone()).await?;
    let cache_for_shutdown = state.cache.clone();

    // Background housekeeping: abort stale multipart uploads (>7 days) and
    // remove leftover temp files from crashed writes. Runs once at startup,
    // then hourly.
    {
        let storage = state.storage.clone();
        tokio::spawn(async move {
            let stale_after = chrono::Duration::days(7);
            let mut ticker = tokio::time::interval(Duration::from_secs(3600));
            loop {
                ticker.tick().await;
                let (uploads, temps) = storage.housekeeping_sweep(stale_after).await;
                if uploads > 0 || temps > 0 {
                    tracing::info!(
                        "housekeeping: removed {} stale upload(s), {} temp file(s)",
                        uploads,
                        temps
                    );
                }
            }
        });
    }

    let app = server::build_router(state);

    let addr = format!("{}:{}", config.address, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    if config.access_key == "maxioadmin" && config.secret_key == "maxioadmin" {
        tracing::warn!(
            "WARNING: Using default credentials because insecure development mode is enabled."
        );
    }

    tracing::info!("MaxIO v{} listening on {}", env!("CARGO_PKG_VERSION"), addr);
    tracing::info!("Access Key: {}", config.access_key);
    tracing::info!("Secret Key: [REDACTED]");
    tracing::info!("Data dir:   {}", config.data_dir);
    let display_host = if config.address == "0.0.0.0" {
        "localhost"
    } else {
        &config.address
    };
    tracing::info!("Web UI:     http://{}:{}/ui/", display_host, config.port);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        if let Some(cache) = cache_for_shutdown {
            if let Err(e) = cache.save_index().await {
                tracing::warn!("shutdown cache index save: {}", e);
            }
            if let Err(e) = cache.flush_dirty().await {
                tracing::warn!("shutdown writeback flush: {}", e);
            }
        }
    })
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    tracing::info!("Shutdown signal received, draining connections...");
}
