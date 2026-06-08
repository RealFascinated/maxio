use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
    map: RwLock<HashMap<String, ObjectMeta>>,
    metrics: Option<Arc<MetricsRegistry>>,
}

impl ObjectReadCache {
    pub fn new(metrics: Option<Arc<MetricsRegistry>>) -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            metrics,
        }
    }

    pub fn get(&self, bucket: &str, key: &str) -> Option<ObjectMeta> {
        let result = self.map.read().ok()?.get(&cache_key(bucket, key)).cloned();
        if result.is_some() {
            if let Some(m) = &self.metrics {
                m.record_cache_hit(cache_name::OBJECT_READ);
            }
        }
        result
    }

    pub fn record_miss(&self) {
        if let Some(m) = &self.metrics {
            m.record_cache_miss(cache_name::OBJECT_READ);
        }
    }

    pub fn insert(&self, bucket: &str, key: &str, meta: ObjectMeta) {
        if let Ok(mut map) = self.map.write() {
            map.insert(cache_key(bucket, key), meta);
            self.sync_entries(map.len());
        }
    }

    pub fn remove(&self, bucket: &str, key: &str) {
        if let Ok(mut map) = self.map.write() {
            if map.remove(&cache_key(bucket, key)).is_some() {
                if let Some(m) = &self.metrics {
                    m.record_cache_eviction(cache_name::OBJECT_READ);
                }
                self.sync_entries(map.len());
            }
        }
    }

    pub fn remove_many(&self, bucket: &str, keys: &[String]) {
        if keys.is_empty() {
            return;
        }
        if let Ok(mut map) = self.map.write() {
            let mut removed = 0u64;
            for key in keys {
                if map.remove(&cache_key(bucket, key)).is_some() {
                    removed += 1;
                }
            }
            if removed > 0 {
                if let Some(m) = &self.metrics {
                    m.record_cache_evictions(cache_name::OBJECT_READ, removed);
                    m.set_cache_entries(cache_name::OBJECT_READ, map.len());
                }
            }
        }
    }

    pub fn remove_bucket(&self, bucket: &str) {
        let prefix = bucket_prefix(bucket);
        if let Ok(mut map) = self.map.write() {
            let before = map.len();
            map.retain(|k, _| !k.starts_with(&prefix));
            let removed = (before - map.len()) as u64;
            if removed > 0 {
                if let Some(m) = &self.metrics {
                    m.record_cache_evictions(cache_name::OBJECT_READ, removed);
                    m.set_cache_entries(cache_name::OBJECT_READ, map.len());
                }
            }
        }
    }

    fn sync_entries(&self, entries: usize) {
        if let Some(m) = &self.metrics {
            m.set_cache_entries(cache_name::OBJECT_READ, entries);
        }
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
        let cache = ObjectReadCache::new(None);
        let meta = sample_meta("obj.txt");
        cache.insert("bench", "obj.txt", meta.clone());

        let cached = cache.get("bench", "obj.txt").expect("entry");
        assert_eq!(cached.key, meta.key);
        assert_eq!(cached.etag, meta.etag);
    }

    #[test]
    fn remove_evicts_entry() {
        let cache = ObjectReadCache::new(None);
        cache.insert("b", "k", sample_meta("k"));
        cache.remove("b", "k");
        assert!(cache.get("b", "k").is_none());
    }

    #[test]
    fn remove_bucket_purges_all_keys_for_bucket() {
        let cache = ObjectReadCache::new(None);
        cache.insert("b1", "a", sample_meta("a"));
        cache.insert("b1", "b", sample_meta("b"));
        cache.insert("b2", "c", sample_meta("c"));

        cache.remove_bucket("b1");

        assert!(cache.get("b1", "a").is_none());
        assert!(cache.get("b1", "b").is_none());
        assert!(cache.get("b2", "c").is_some());
    }
}
