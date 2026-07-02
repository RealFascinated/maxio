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

    /// Max object read-metadata cache entries (MAXIO_OBJECT_READ_CACHE_MAX_ENTRIES).
    #[arg(
        long,
        env = "MAXIO_OBJECT_READ_CACHE_MAX_ENTRIES",
        default_value = "262144"
    )]
    pub object_read_cache_max_entries: usize,

    /// Max bucket metadata cache entries (MAXIO_BUCKET_CACHE_MAX_ENTRIES).
    #[arg(long, env = "MAXIO_BUCKET_CACHE_MAX_ENTRIES", default_value = "10000")]
    pub bucket_cache_max_entries: usize,

    /// Max in-flight multipart session cache entries (MAXIO_MULTIPART_CACHE_MAX_ENTRIES).
    #[arg(
        long,
        env = "MAXIO_MULTIPART_CACHE_MAX_ENTRIES",
        default_value = "32768"
    )]
    pub multipart_cache_max_entries: usize,

    /// Max signing key cache entries (MAXIO_SIGNING_KEY_CACHE_MAX_ENTRIES).
    #[arg(
        long,
        env = "MAXIO_SIGNING_KEY_CACHE_MAX_ENTRIES",
        default_value = "10000"
    )]
    pub signing_key_cache_max_entries: usize,

    /// Max entries per IAM metadata sub-cache (MAXIO_IAM_CACHE_MAX_ENTRIES).
    #[arg(long, env = "MAXIO_IAM_CACHE_MAX_ENTRIES", default_value = "10000")]
    pub iam_cache_max_entries: usize,

    /// Public S3 base URL for presigned links (MAXIO_PUBLIC_URL), e.g. https://s3.example.com
    #[arg(long, env = "MAXIO_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Max Postgres connection pool size (MAXIO_DB_POOL_SIZE).
    #[arg(long, env = "MAXIO_DB_POOL_SIZE", default_value = "64")]
    pub db_pool_size: u32,

    /// Cache prepared SQL statements on each pool connection (MAXIO_DB_PREPARED_STATEMENT_CACHE).
    #[arg(
        long,
        env = "MAXIO_DB_PREPARED_STATEMENT_CACHE",
        default_value = "true"
    )]
    pub db_prepared_statement_cache: bool,
}

/// Postgres pool tuning passed to [`crate::db::create_pool`].
#[derive(Debug, Clone, Copy)]
pub struct PoolSettings {
    pub max_size: u32,
    pub prepared_statement_cache: bool,
}

impl Default for PoolSettings {
    fn default() -> Self {
        Self {
            max_size: 64,
            prepared_statement_cache: true,
        }
    }
}

impl From<&Config> for PoolSettings {
    fn from(config: &Config) -> Self {
        Self {
            max_size: config.db_pool_size,
            prepared_statement_cache: config.db_prepared_statement_cache,
        }
    }
}

/// Entry limits for in-memory metadata caches.
#[derive(Debug, Clone, Copy)]
pub struct MemoryCacheLimits {
    pub object_read_max_entries: usize,
    pub bucket_max_entries: usize,
    pub multipart_max_entries: usize,
    pub signing_key_max_entries: usize,
    pub iam_max_entries: usize,
}

impl Default for MemoryCacheLimits {
    fn default() -> Self {
        Self {
            object_read_max_entries: 262_144,
            bucket_max_entries: 10_000,
            multipart_max_entries: 32_768,
            signing_key_max_entries: 10_000,
            iam_max_entries: 10_000,
        }
    }
}

impl From<&Config> for MemoryCacheLimits {
    fn from(config: &Config) -> Self {
        Self {
            object_read_max_entries: config.object_read_cache_max_entries,
            bucket_max_entries: config.bucket_cache_max_entries,
            multipart_max_entries: config.multipart_cache_max_entries,
            signing_key_max_entries: config.signing_key_cache_max_entries,
            iam_max_entries: config.iam_cache_max_entries,
        }
    }
}
