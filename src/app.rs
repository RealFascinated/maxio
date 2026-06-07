use std::sync::Arc;

use crate::api::console::LoginRateLimiter;
use crate::config::Config;
use crate::db;
use crate::iam::{IamStore, PgIamStore};
use crate::metrics::MetricsRegistry;
use crate::server::AppState;
use crate::stats::BucketStatsCache;
use crate::storage::blob::BlobStorage;
use crate::storage::{MetadataStore, ObjectStorage, PgMetadataStore, Storage};

pub async fn build_app_state(config: Config) -> anyhow::Result<AppState> {
    tokio::fs::create_dir_all(&config.data_dir).await?;

    db::run_migrations(&config.database_url).await?;
    let pool = db::create_pool(&config.database_url).await?;
    let pool = Arc::new(pool);

    let metrics = Arc::new(MetricsRegistry::new()?);

    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(pool.clone()));
    let blobs = BlobStorage::new(&config.data_dir).await?;
    let storage: Arc<dyn Storage> =
        Arc::new(ObjectStorage::new(blobs, meta).with_metrics(Arc::clone(&metrics)));

    crate::storage::provision_default_buckets(
        storage.as_ref(),
        &config.default_buckets,
        &config.region,
    )
    .await;

    let user_store: Arc<dyn IamStore> = Arc::new(PgIamStore::new(pool.clone()));
    let stats = BucketStatsCache::new(Arc::clone(&pool), Arc::clone(&metrics));

    Ok(AppState {
        storage,
        config: Arc::new(config),
        login_rate_limiter: Arc::new(LoginRateLimiter::new()),
        user_store,
        db_pool: pool,
        metrics,
        stats,
    })
}
