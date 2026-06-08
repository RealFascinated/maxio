use std::sync::Arc;

use super::DbPool;
use super::bucket_cache::BucketCache;
use super::object_read_cache::ObjectReadCache;
use crate::metrics::MetricsRegistry;

/// Shared database pool plus process-local caches for metadata hot paths.
#[derive(Clone)]
pub struct DbContext {
    pool: Arc<DbPool>,
    bucket_cache: Arc<BucketCache>,
    object_read_cache: Arc<ObjectReadCache>,
}

impl DbContext {
    pub fn new(pool: Arc<DbPool>, metrics: Option<Arc<MetricsRegistry>>) -> Self {
        Self {
            pool,
            bucket_cache: Arc::new(BucketCache::new(metrics.clone())),
            object_read_cache: Arc::new(ObjectReadCache::new(metrics)),
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

    pub fn object_read_cache(&self) -> &ObjectReadCache {
        &self.object_read_cache
    }
}
