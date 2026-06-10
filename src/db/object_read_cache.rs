use std::sync::Arc;

use crate::cache::MetricsLruCache;
use crate::metrics::{MetricsRegistry, cache_name};
use crate::storage::ObjectMeta;

fn cache_key(bucket: &str, key: &str) -> String {
    format!("{bucket}\0{key}")
}

fn bucket_prefix(bucket: &str) -> String {
    format!("{bucket}\0")
}

#[derive(Debug, Clone)]
enum ReadCacheValue {
    Meta(Box<ObjectMeta>),
    Absent,
}

/// Result of a read-cache lookup before hitting Postgres.
#[derive(Debug, Clone)]
pub enum ReadCacheLookup {
    Hit(Box<ObjectMeta>),
    Absent,
    Miss,
}

/// In-memory cache of current-object read metadata for GET/HEAD hot paths.
pub struct ObjectReadCache {
    map: MetricsLruCache<String, ReadCacheValue>,
}

impl ObjectReadCache {
    pub fn new(metrics: Option<Arc<MetricsRegistry>>, max_entries: usize) -> Self {
        Self {
            map: MetricsLruCache::new(metrics, cache_name::OBJECT_READ, max_entries),
        }
    }

    pub fn lookup(&self, bucket: &str, key: &str) -> ReadCacheLookup {
        match self.map.get(&cache_key(bucket, key)) {
            Some(ReadCacheValue::Meta(meta)) => ReadCacheLookup::Hit(meta),
            Some(ReadCacheValue::Absent) => ReadCacheLookup::Absent,
            None => ReadCacheLookup::Miss,
        }
    }

    pub fn record_miss(&self) {
        self.map.record_miss();
    }

    pub fn insert(&self, bucket: &str, key: &str, meta: ObjectMeta) {
        self.map
            .insert(cache_key(bucket, key), ReadCacheValue::Meta(Box::new(meta)));
    }

    pub fn mark_absent(&self, bucket: &str, key: &str) {
        self.map
            .insert(cache_key(bucket, key), ReadCacheValue::Absent);
    }

    pub fn mark_absent_many(&self, bucket: &str, keys: &[String]) {
        for key in keys {
            self.mark_absent(bucket, key);
        }
    }

    pub fn remove_bucket(&self, bucket: &str) {
        let prefix = bucket_prefix(bucket);
        self.map.remove_where(|key| key.starts_with(&prefix));
    }
}
