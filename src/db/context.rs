use std::sync::Arc;

use super::DbPool;
use super::bucket_cache::BucketCache;
use super::multipart_cache::MultipartCache;
use super::object_read_cache::ObjectReadCache;
use crate::config::MemoryCacheLimits;
use crate::metrics::MetricsRegistry;

/// Shared database pool plus process-local caches for metadata hot paths.
#[derive(Clone)]
pub struct DbContext {
    pool: Arc<DbPool>,
    bucket_cache: Arc<BucketCache>,
    multipart_cache: Arc<MultipartCache>,
    object_read_cache: Arc<ObjectReadCache>,
}

impl DbContext {
    pub fn new(
        pool: Arc<DbPool>,
        metrics: Option<Arc<MetricsRegistry>>,
        limits: MemoryCacheLimits,
    ) -> Self {
        Self {
            pool,
            bucket_cache: Arc::new(BucketCache::new(metrics.clone(), limits.bucket_max_entries)),
            multipart_cache: Arc::new(MultipartCache::new(
                metrics.clone(),
                limits.multipart_max_entries,
            )),
            object_read_cache: Arc::new(ObjectReadCache::new(
                metrics,
                limits.object_read_max_entries,
            )),
        }
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub fn pool_arc(&self) -> Arc<DbPool> {
        Arc::clone(&self.pool)
    }

    pub fn bucket_cache(&self) -> &BucketCache {
        &self.bucket_cache
    }

    pub fn multipart_cache(&self) -> &MultipartCache {
        &self.multipart_cache
    }

    pub fn object_read_cache(&self) -> &ObjectReadCache {
        &self.object_read_cache
    }
}
