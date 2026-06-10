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
    Meta(ObjectMeta),
    Absent,
}

/// Result of a read-cache lookup before hitting Postgres.
#[derive(Debug, Clone)]
pub enum ReadCacheLookup {
    Hit(ObjectMeta),
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

    pub fn get(&self, bucket: &str, key: &str) -> Option<ObjectMeta> {
        match self.lookup(bucket, key) {
            ReadCacheLookup::Hit(meta) => Some(meta),
            _ => None,
        }
    }

    pub fn record_miss(&self) {
        self.map.record_miss();
    }

    pub fn insert(&self, bucket: &str, key: &str, meta: ObjectMeta) {
        self.map
            .insert(cache_key(bucket, key), ReadCacheValue::Meta(meta));
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

    pub fn remove(&self, bucket: &str, key: &str) {
        self.map.remove(&cache_key(bucket, key));
    }

    pub fn remove_many(&self, bucket: &str, keys: &[String]) {
        let keys: Vec<String> = keys.iter().map(|key| cache_key(bucket, key)).collect();
        self.map.remove_many(&keys);
    }

    pub fn remove_bucket(&self, bucket: &str) {
        let prefix = bucket_prefix(bucket);
        self.map.remove_where(|key| key.starts_with(&prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_meta(key: &str) -> ObjectMeta {
        ObjectMeta {
            key: key.to_string(),
            size: 42,
            etag: "\"abc\"".into(),
            content_type: "text/plain".into(),
            last_modified: "Mon, 01 Jan 2024 00:00:00 GMT".into(),
            owner_id: "owner".into(),
            owner_display_name: "Owner".into(),
            acl: None,
            version_id: None,
            is_delete_marker: false,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
        }
    }

    #[test]
    fn caches_and_returns_read_meta() {
        let cache = ObjectReadCache::new(None, 256);
        let meta = sample_meta("obj.txt");
        cache.insert("bench", "obj.txt", meta.clone());

        assert!(matches!(
            cache.lookup("bench", "obj.txt"),
            ReadCacheLookup::Hit(hit) if hit.key == meta.key && hit.etag == meta.etag
        ));
    }

    #[test]
    fn absent_tombstone_skips_db() {
        let cache = ObjectReadCache::new(None, 256);
        cache.mark_absent("bench", "gone.txt");
        assert!(matches!(
            cache.lookup("bench", "gone.txt"),
            ReadCacheLookup::Absent
        ));
    }

    #[test]
    fn insert_clears_absent_tombstone() {
        let cache = ObjectReadCache::new(None, 256);
        cache.mark_absent("bench", "obj.txt");
        let meta = sample_meta("obj.txt");
        cache.insert("bench", "obj.txt", meta.clone());
        assert!(matches!(
            cache.lookup("bench", "obj.txt"),
            ReadCacheLookup::Hit(hit) if hit.key == meta.key
        ));
    }

    #[test]
    fn remove_evicts_entry() {
        let cache = ObjectReadCache::new(None, 256);
        cache.insert("b", "k", sample_meta("k"));
        cache.remove("b", "k");
        assert!(matches!(cache.lookup("b", "k"), ReadCacheLookup::Miss));
    }

    #[test]
    fn remove_bucket_purges_all_keys_for_bucket() {
        let cache = ObjectReadCache::new(None, 256);
        cache.insert("b1", "a", sample_meta("a"));
        cache.insert("b1", "b", sample_meta("b"));
        cache.insert("b2", "c", sample_meta("c"));
        cache.mark_absent("b1", "missing");

        cache.remove_bucket("b1");

        assert!(matches!(cache.lookup("b1", "a"), ReadCacheLookup::Miss));
        assert!(matches!(cache.lookup("b1", "b"), ReadCacheLookup::Miss));
        assert!(matches!(
            cache.lookup("b1", "missing"),
            ReadCacheLookup::Miss
        ));
        assert!(matches!(cache.lookup("b2", "c"), ReadCacheLookup::Hit(_)));
    }

    #[test]
    fn evicts_lru_when_at_capacity() {
        let cache = ObjectReadCache::new(None, 2);
        cache.insert("b", "first", sample_meta("first"));
        cache.insert("b", "second", sample_meta("second"));
        cache.get("b", "first");
        cache.insert("b", "third", sample_meta("third"));

        assert!(matches!(
            cache.lookup("b", "first"),
            ReadCacheLookup::Hit(_)
        ));
        assert!(matches!(cache.lookup("b", "second"), ReadCacheLookup::Miss));
        assert!(matches!(
            cache.lookup("b", "third"),
            ReadCacheLookup::Hit(_)
        ));
    }
}
