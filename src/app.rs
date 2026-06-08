use std::sync::Arc;
use std::time::Duration;

use crate::api::console::LoginRateLimiter;
use crate::config::Config;
use crate::db;
use crate::iam::{IamStore, PgIamStore};
use crate::metrics::MetricsRegistry;
use crate::server::AppState;
use crate::stats::BucketStatsCache;
use crate::storage::blob::BlobStorage;
use crate::storage::cache::CacheLayer;
use crate::storage::{MetadataStore, ObjectStorage, PgMetadataStore, Storage};

pub async fn build_app_state(config: Config) -> anyhow::Result<AppState> {
    tokio::fs::create_dir_all(&config.data_dir).await?;

    db::run_migrations(&config.database_url).await?;
    let pool = db::create_pool(&config.database_url).await?;
    let pool = Arc::new(pool);

    let metrics = Arc::new(MetricsRegistry::new()?);

    let meta: Arc<dyn MetadataStore> =
        Arc::new(PgMetadataStore::new(pool.clone()).with_metrics(Arc::clone(&metrics)));
    let blobs = BlobStorage::new(&config.data_dir).await?;
    let mut cache_handle = None;
    let blobs = if let Some(cache_dir) = &config.cache_dir {
        let data_buckets_dir = blobs.buckets_dir.clone();
        let cache = CacheLayer::new(
            cache_dir,
            data_buckets_dir,
            config.cache_max_size,
            config.cache_writeback,
            Duration::from_secs(config.cache_flush_interval),
        )
        .await?
        .with_metrics(Arc::clone(&metrics));
        metrics.init_cache_metrics(config.cache_max_size);
        let cache = Arc::new(cache);
        cache_handle = Some(Arc::clone(&cache));
        cache.clone().spawn_flush_task();
        blobs.with_cache(cache)
    } else {
        blobs
    };
    let storage: Arc<dyn Storage> =
        Arc::new(ObjectStorage::new(blobs, meta).with_metrics(Arc::clone(&metrics)));

    crate::storage::provision_default_buckets(storage.as_ref(), &config.default_buckets).await;

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
        cache: cache_handle,
    })
}
