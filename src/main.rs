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
mod config;
mod db;
mod iam;
mod embedded;
mod error;
mod server;
mod storage;
mod xml;

use clap::Parser;
use clap::Subcommand;
use config::Config;
use http::Uri;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "maxio",
    about = "S3-compatible object storage server",
    version = env!("CARGO_PKG_VERSION")
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    config: Config,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the HTTP/S3 server (default when no subcommand is provided)
    Serve,

    /// Check server health by sending an HTTP GET request
    Healthcheck {
        /// Healthcheck endpoint URL
        #[arg(long, env = "MAXIO_HEALTHCHECK_URL", default_value_t = default_healthcheck_url())]
        url: String,

        /// Timeout in milliseconds for connect/read operations
        #[arg(long, env = "MAXIO_HEALTHCHECK_TIMEOUT_MS", default_value = "2000")]
        timeout_ms: u64,
    },


    /// Manage IAM users
    #[command(subcommand)]
    User(UserCmd),

    /// Manage IAM policies
    #[command(subcommand)]
    Policy(PolicyCmd),
}

#[derive(Subcommand, Debug)]
enum UserCmd {
    Add {
        #[arg(long)]
        username: String,
        #[arg(long)]
        access_key: Option<String>,
        #[arg(long)]
        secret_key: Option<String>,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    List {
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    Delete {
        #[arg(long)]
        username: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    CreateKey {
        #[arg(long)]
        username: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    DeleteKey {
        #[arg(long)]
        username: String,
        #[arg(long)]
        access_key_id: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    PutPolicy {
        #[arg(long)]
        username: String,
        #[arg(long)]
        policy_name: String,
        #[arg(long)]
        document: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    AttachPolicy {
        #[arg(long)]
        username: String,
        #[arg(long)]
        policy_arn: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
}

#[derive(Subcommand, Debug)]
enum PolicyCmd {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        document: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    List {
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    Show {
        #[arg(long)]
        name: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    Delete {
        #[arg(long)]
        name: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
}


fn default_healthcheck_url() -> String {
    let port = std::env::var("MAXIO_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(9000);
    format!("http://127.0.0.1:{}/healthz", port)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Serve) | None => {}
        Some(Commands::Healthcheck { url, timeout_ms }) => {
            return run_healthcheck(&url, timeout_ms).await;
        }
        Some(Commands::User(cmd)) => return run_user_cmd(cmd).await,
        Some(Commands::Policy(cmd)) => return run_policy_cmd(cmd).await,
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = cli.config;

    if config.access_key == "maxioadmin"
        && config.secret_key == "maxioadmin"
        && !config.allow_insecure_dev
    {
        anyhow::bail!(
            "refusing to start with default credentials in production; set MAXIO_ACCESS_KEY/MAXIO_SECRET_KEY or use --allow-insecure-dev for local development"
        );
    }

    let state = app::build_app_state(config.clone()).await?;

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
    tracing::info!("Region:     {}", config.region);
    if config.erasure_coding {
        tracing::info!(
            "Erasure coding: enabled (chunk size: {}MB)",
            config.chunk_size / (1024 * 1024)
        );
        if config.parity_shards > 0 {
            tracing::info!(
                "Parity shards: {} (can tolerate {} lost/corrupt chunks per object)",
                config.parity_shards,
                config.parity_shards
            );
        }
    } else if config.parity_shards > 0 {
        tracing::warn!("--parity-shards ignored: requires --erasure-coding to be enabled");
    }
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
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn run_healthcheck(url: &str, timeout_ms: u64) -> anyhow::Result<()> {
    let uri: Uri = url.parse()?;
    if uri.scheme_str() != Some("http") {
        anyhow::bail!("unsupported scheme in healthcheck URL: only http is supported");
    }

    let host = uri
        .host()
        .ok_or_else(|| anyhow::anyhow!("healthcheck URL is missing host"))?;
    let port = uri.port_u16().unwrap_or(80);
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let timeout_duration = Duration::from_millis(timeout_ms);

    let mut stream: TcpStream = timeout(timeout_duration, TcpStream::connect((host, port)))
        .await
        .map_err(|_| anyhow::anyhow!("healthcheck connect timeout after {}ms", timeout_ms))??;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: maxio-healthcheck/{}\r\n\r\n",
        path_and_query,
        host,
        env!("CARGO_PKG_VERSION")
    );
    timeout(timeout_duration, stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| anyhow::anyhow!("healthcheck write timeout after {}ms", timeout_ms))??;

    let mut response = Vec::new();
    timeout(timeout_duration, stream.read_to_end(&mut response))
        .await
        .map_err(|_| anyhow::anyhow!("healthcheck read timeout after {}ms", timeout_ms))??;

    let status_line = String::from_utf8_lossy(&response)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP response from {}", url))?;

    if (200..300).contains(&status_code) {
        println!("ok");
        return Ok(());
    }

    anyhow::bail!("healthcheck failed with HTTP status {}", status_code);
}


async fn load_iam_store(database_url: &str) -> anyhow::Result<Arc<dyn iam::IamStore>> {
    let pool = db::create_pool(database_url).await?;
    db::run_migrations(database_url).await?;
    Ok(Arc::new(iam::PgIamStore::new(Arc::new(pool))))
}

async fn run_user_cmd(cmd: UserCmd) -> anyhow::Result<()> {
    use iam::types::PolicyDocumentRaw;
    match cmd {
        UserCmd::Add {
            username,
            access_key,
            secret_key,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            api::iam::cli_add_user(store.as_ref(), &username, access_key.as_deref(), secret_key.as_deref())
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ user {username} created");
        }
        UserCmd::List { database_url } => {
            let store = load_iam_store(&database_url).await?;
            for u in store.list_users().await {
                println!("{} ({}) keys={}", u.username, u.user_id, u.access_keys.len());
            }
        }
        UserCmd::Delete { username, database_url } => {
            let store = load_iam_store(&database_url).await?;
            store.delete_user(&username).await.map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ user {username} deleted");
        }
        UserCmd::CreateKey { username, database_url } => {
            let store = load_iam_store(&database_url).await?;
            let key = store
                .create_access_key(&username)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("access_key_id={}", key.access_key_id);
            println!("secret_access_key={}", key.secret_access_key);
        }
        UserCmd::DeleteKey {
            username,
            access_key_id,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            store
                .delete_access_key(&username, &access_key_id)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ access key deleted");
        }
        UserCmd::PutPolicy {
            username,
            policy_name,
            document,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            let doc: PolicyDocumentRaw = serde_json::from_str(&document)?;
            store
                .put_user_policy(&username, &policy_name, doc)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ inline policy attached");
        }
        UserCmd::AttachPolicy {
            username,
            policy_arn,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            store
                .attach_user_policy(&username, &policy_arn)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ managed policy attached");
        }
    }
    Ok(())
}

async fn run_policy_cmd(cmd: PolicyCmd) -> anyhow::Result<()> {
    use iam::types::PolicyDocumentRaw;
    match cmd {
        PolicyCmd::Create {
            name,
            document,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            let doc: PolicyDocumentRaw = serde_json::from_str(&document)?;
            let policy = store
                .create_managed_policy(&name, doc)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ policy {} ({})", policy.policy_name, policy.arn);
        }
        PolicyCmd::List { database_url } => {
            let store = load_iam_store(&database_url).await?;
            for p in store.list_managed_policies().await {
                println!("{} {}", p.policy_name, p.arn);
            }
        }
        PolicyCmd::Show { name, database_url } => {
            let store = load_iam_store(&database_url).await?;
            let policy = store
                .get_managed_policy(&name)
                .await
                .ok_or_else(|| anyhow::anyhow!("policy not found"))?;
            println!("{}", serde_json::to_string_pretty(&policy.document)?);
        }
        PolicyCmd::Delete { name, database_url } => {
            let store = load_iam_store(&database_url).await?;
            store
                .delete_managed_policy(&name)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ policy deleted");
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    tracing::info!("Shutdown signal received, draining connections...");
}
