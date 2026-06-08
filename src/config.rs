use clap::Args;
use std::env;

fn first_env_value(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
}

fn default_access_key() -> String {
    first_env_value(&["MINIO_ROOT_USER", "MINIO_ACCESS_KEY"])
        .unwrap_or_else(|| "maxioadmin".to_string())
}

fn default_secret_key() -> String {
    first_env_value(&["MINIO_ROOT_PASSWORD", "MINIO_SECRET_KEY"])
        .unwrap_or_else(|| "maxioadmin".to_string())
}

fn default_default_buckets() -> Option<String> {
    first_env_value(&["MINIO_DEFAULT_BUCKETS"])
}

#[derive(Args, Debug, Clone)]
pub struct Config {
    /// Port to listen on
    #[arg(long, env = "MAXIO_PORT", default_value = "9000")]
    pub port: u16,

    /// Address to bind to
    #[arg(long, env = "MAXIO_ADDRESS", default_value = "0.0.0.0")]
    pub address: String,

    /// Root data directory (object bytes on disk)
    #[arg(long, env = "MAXIO_DATA_DIR", default_value = "./data")]
    pub data_dir: String,

    /// PostgreSQL connection URL for metadata
    #[arg(long, env = "MAXIO_DATABASE_URL")]
    pub database_url: String,

    /// Access key (MAXIO_ACCESS_KEY, MINIO_ROOT_USER, MINIO_ACCESS_KEY)
    #[arg(long, env = "MAXIO_ACCESS_KEY", default_value_t = default_access_key())]
    pub access_key: String,

    /// Secret key (MAXIO_SECRET_KEY, MINIO_ROOT_PASSWORD, MINIO_SECRET_KEY)
    #[arg(long, env = "MAXIO_SECRET_KEY", default_value_t = default_secret_key())]
    pub secret_key: String,

    /// Allow insecure development defaults (default credentials, HTTP cookies).
    #[arg(long, env = "MAXIO_ALLOW_INSECURE_DEV", default_value = "false")]
    pub allow_insecure_dev: bool,

    /// Force Secure on console session cookies. Keep enabled for public consoles.
    #[arg(long, env = "MAXIO_SECURE_COOKIES", default_value = "true")]
    pub secure_cookies: bool,

    /// Comma-separated list of bucket names to create on first boot
    /// (MAXIO_DEFAULT_BUCKETS, MINIO_DEFAULT_BUCKETS)
    #[arg(long, env = "MAXIO_DEFAULT_BUCKETS", default_value_t = default_default_buckets().unwrap_or_default())]
    pub default_buckets: String,

    /// Max request body size for console JSON/form API routes, in bytes. Object uploads are streaming and not covered by this limit.
    #[arg(long, env = "MAXIO_MAX_CONSOLE_BODY_BYTES", default_value = "1048576")]
    pub max_console_body_bytes: usize,

    /// Bearer token required to scrape GET /metrics (MAXIO_METRICS_TOKEN).
    /// When empty the endpoint returns 403 Forbidden (metrics disabled).
    #[arg(long, env = "MAXIO_METRICS_TOKEN", default_value = "")]
    pub metrics_token: String,

    /// Optional SSD cache directory for object bytes (MAXIO_CACHE_DIR).
    #[arg(long, env = "MAXIO_CACHE_DIR")]
    pub cache_dir: Option<String>,

    /// Maximum cache size in bytes (MAXIO_CACHE_MAX_SIZE). Default 10 GiB.
    #[arg(long, env = "MAXIO_CACHE_MAX_SIZE", default_value = "10737418240")]
    pub cache_max_size: u64,

    /// Write to cache first and flush to data_dir in the background (MAXIO_CACHE_WRITEBACK).
    #[arg(long, env = "MAXIO_CACHE_WRITEBACK", default_value = "false")]
    pub cache_writeback: bool,

    /// Writeback flush interval in seconds (MAXIO_CACHE_FLUSH_INTERVAL).
    #[arg(long, env = "MAXIO_CACHE_FLUSH_INTERVAL", default_value = "30")]
    pub cache_flush_interval: u64,

    /// Public S3 base URL for presigned links (MAXIO_PUBLIC_URL), e.g. https://s3.example.com
    #[arg(long, env = "MAXIO_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Return 200 after bytes are durable on disk, commit metadata to Postgres in the background.
    /// Improves PUT throughput significantly at the cost of a narrow inconsistency window on crash.
    /// Incompatible with bucket versioning (versioning takes the synchronous path regardless).
    #[arg(long, env = "MAXIO_ASYNC_META_WRITE", default_value = "false")]
    pub async_meta_write: bool,
}

#[cfg(test)]
mod tests {
    use super::Config;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(flatten)]
        config: Config,
    }

    #[test]
    fn default_address_is_all_interfaces() {
        unsafe {
            std::env::remove_var("MAXIO_ADDRESS");
        }

        let cli = TestCli::parse_from(["maxio", "--database-url", "postgres://localhost/maxio"]);

        assert_eq!(cli.config.address, "0.0.0.0");
    }
}
