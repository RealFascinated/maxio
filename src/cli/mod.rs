mod healthcheck;
mod orphan_meta;
mod policy;
mod user;

use clap::{Parser, Subcommand};

use crate::config::Config;

use policy::PolicyCmd;
use user::UserCmd;

#[derive(Parser, Debug)]
#[command(
    name = "maxio",
    about = "S3-compatible object storage server",
    version = env!("CARGO_PKG_VERSION")
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub config: Config,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
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

    /// List metadata rows whose object bytes are missing on disk
    OrphanMeta {
        /// Delete listed orphaned metadata rows
        #[arg(long)]
        delete: bool,

        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,

        #[arg(long, env = "MAXIO_DATA_DIR", default_value = "./data")]
        data_dir: String,

        /// Also check SSD cache directory when configured
        #[arg(long, env = "MAXIO_CACHE_DIR")]
        cache_dir: Option<String>,
    },
}

fn default_healthcheck_url() -> String {
    let port = std::env::var("MAXIO_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(9000);
    format!("http://127.0.0.1:{}/healthz", port)
}

/// Run a CLI subcommand. Returns `Some(config)` when the server should start.
pub async fn execute(cli: Cli) -> anyhow::Result<Option<Config>> {
    match cli.command {
        Some(Commands::Serve) | None => Ok(Some(cli.config)),
        Some(Commands::Healthcheck { url, timeout_ms }) => {
            healthcheck::run(&url, timeout_ms).await?;
            Ok(None)
        }
        Some(Commands::User(cmd)) => {
            user::run(cmd).await?;
            Ok(None)
        }
        Some(Commands::Policy(cmd)) => {
            policy::run(cmd).await?;
            Ok(None)
        }
        Some(Commands::OrphanMeta {
            delete,
            database_url,
            data_dir,
            cache_dir,
        }) => {
            orphan_meta::run(delete, &database_url, &data_dir, cache_dir.as_deref()).await?;
            Ok(None)
        }
    }
}

pub(crate) async fn load_iam_store(
    database_url: &str,
) -> anyhow::Result<std::sync::Arc<dyn crate::iam::IamStore>> {
    let pool = crate::db::create_pool(database_url).await?;
    crate::db::run_migrations(database_url).await?;
    Ok(std::sync::Arc::new(crate::iam::PgIamStore::new(
        std::sync::Arc::new(pool),
    )))
}
