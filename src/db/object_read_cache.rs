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

/// In-memory cache of current-object read metadata for GET/HEAD hot paths.
pub struct ObjectReadCache {
    map: MetricsLruCache<String, ObjectMeta>,
}

impl ObjectReadCache {
    pub fn new(metrics: Option<Arc<MetricsRegistry>>, max_entries: usize) -> Self {
        Self {
            map: MetricsLruCache::new(metrics, cache_name::OBJECT_READ, max_entries),
        }
    }

    pub fn get(&self, bucket: &str, key: &str) -> Option<ObjectMeta> {
        self.map.get(&cache_key(bucket, key))
    }

    pub fn record_miss(&self) {
        self.map.record_miss();
    }

    pub fn insert(&self, bucket: &str, key: &str, meta: ObjectMeta) {
        self.map.insert(cache_key(bucket, key), meta);
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

        let cached = cache.get("bench", "obj.txt").expect("entry");
        assert_eq!(cached.key, meta.key);
        assert_eq!(cached.etag, meta.etag);
    }

    #[test]
    fn remove_evicts_entry() {
        let cache = ObjectReadCache::new(None, 256);
        cache.insert("b", "k", sample_meta("k"));
        cache.remove("b", "k");
        assert!(cache.get("b", "k").is_none());
    }

    #[test]
    fn remove_bucket_purges_all_keys_for_bucket() {
        let cache = ObjectReadCache::new(None, 256);
        cache.insert("b1", "a", sample_meta("a"));
        cache.insert("b1", "b", sample_meta("b"));
        cache.insert("b2", "c", sample_meta("c"));

        cache.remove_bucket("b1");

        assert!(cache.get("b1", "a").is_none());
        assert!(cache.get("b1", "b").is_none());
        assert!(cache.get("b2", "c").is_some());
    }

    #[test]
    fn evicts_lru_when_at_capacity() {
        let cache = ObjectReadCache::new(None, 2);
        cache.insert("b", "first", sample_meta("first"));
        cache.insert("b", "second", sample_meta("second"));
        cache.get("b", "first");
        cache.insert("b", "third", sample_meta("third"));

        assert!(cache.get("b", "first").is_some());
        assert!(cache.get("b", "second").is_none());
        assert!(cache.get("b", "third").is_some());
    }
}
