use std::sync::Arc;

use super::DbPool;
use super::bucket_cache::BucketCache;

/// Shared database pool plus process-local caches for metadata hot paths.
#[derive(Clone)]
pub struct DbContext {
    pool: Arc<DbPool>,
    bucket_cache: Arc<BucketCache>,
}

impl DbContext {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self {
            pool,
            bucket_cache: Arc::new(BucketCache::new()),
        }
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub fn bucket_cache(&self) -> &BucketCache {
        &self.bucket_cache
    }
}
