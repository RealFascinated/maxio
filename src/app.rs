use std::sync::Arc;

use crate::api::console::LoginRateLimiter;
use crate::config::Config;
use crate::db::{self, DbPool};
use crate::iam::{IamStore, PgIamStore};
use crate::server::AppState;
use crate::storage::blob::BlobStorage;
use crate::storage::{MetadataStore, ObjectStorage, PgMetadataStore, Storage};

pub async fn build_app_state(config: Config) -> anyhow::Result<AppState> {
    tokio::fs::create_dir_all(&config.data_dir).await?;

    db::run_migrations(&config.database_url).await?;
    let pool = db::create_pool(&config.database_url).await?;
    let pool = Arc::new(pool);

    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(pool.clone()));
    let blobs = BlobStorage::new(
        &config.data_dir,
        config.erasure_coding,
        config.chunk_size,
        config.parity_shards,
    )
    .await?;
    let storage: Arc<dyn Storage> = Arc::new(ObjectStorage::new(blobs, meta));

    crate::storage::provision_default_buckets(
        storage.as_ref(),
        &config.default_buckets,
        &config.region,
    )
    .await;

    let user_store: Arc<dyn IamStore> = Arc::new(PgIamStore::new(pool.clone()));

    Ok(AppState {
        storage,
        config: Arc::new(config),
        login_rate_limiter: Arc::new(LoginRateLimiter::new()),
        user_store,
        db_pool: pool,
    })
}

pub async fn create_storage_from_config(config: &Config) -> anyhow::Result<Arc<dyn Storage>> {
    db::run_migrations(&config.database_url).await?;
    let pool = db::create_pool(&config.database_url).await?;
    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(Arc::new(pool)));
    let blobs = BlobStorage::new(
        &config.data_dir,
        config.erasure_coding,
        config.chunk_size,
        config.parity_shards,
    )
    .await?;
    Ok(Arc::new(ObjectStorage::new(blobs, meta)))
}

pub type SharedPool = Arc<DbPool>;
