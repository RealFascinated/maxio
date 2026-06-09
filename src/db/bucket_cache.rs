use std::sync::Arc;

use uuid::Uuid;

use crate::cache::MetricsLruCache;
use crate::iam::Acl;
use crate::metrics::{MetricsRegistry, cache_name};
use crate::storage::CorsRule;

/// Cached bucket metadata for hot paths (PutObject, auth, bucket id resolution, CORS).
#[derive(Debug, Clone)]
pub struct CachedBucketEntry {
    pub id: Uuid,
    pub versioning: bool,
    pub owner_id: String,
    pub owner_display_name: String,
    pub policy: Option<String>,
    pub acl: Option<Acl>,
    pub cors_rules: Vec<CorsRule>,
}

/// In-memory bucket → metadata cache to avoid repeated Postgres lookups.
pub struct BucketCache {
    map: MetricsLruCache<String, CachedBucketEntry>,
}

impl BucketCache {
    pub fn new(metrics: Option<Arc<MetricsRegistry>>, max_entries: usize) -> Self {
        Self {
            map: MetricsLruCache::new(metrics, cache_name::BUCKET, max_entries),
        }
    }

    pub fn get(&self, name: &str) -> Option<CachedBucketEntry> {
        self.map.get(name)
    }

    pub fn record_miss(&self) {
        self.map.record_miss();
    }

    pub fn insert(&self, name: impl Into<String>, entry: CachedBucketEntry) {
        self.map.insert(name.into(), entry);
    }

    pub fn remove(&self, name: &str) {
        self.map.remove(name);
    }

    pub fn set_versioning(&self, name: &str, enabled: bool) {
        self.map.get_mut(name, |entry| entry.versioning = enabled);
    }

    pub fn set_policy(&self, name: &str, policy: Option<String>) {
        self.map.get_mut(name, |entry| entry.policy = policy);
    }

    pub fn set_acl(&self, name: &str, acl: Option<Acl>) {
        self.map.get_mut(name, |entry| entry.acl = acl);
    }

    pub fn set_cors(&self, name: &str, rules: Vec<CorsRule>) {
        self.map.get_mut(name, |entry| entry.cors_rules = rules);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> CachedBucketEntry {
        CachedBucketEntry {
            id: Uuid::new_v4(),
            versioning: false,
            owner_id: "owner".into(),
            owner_display_name: "Owner".into(),
            policy: None,
            acl: None,
            cors_rules: vec![],
        }
    }

    #[test]
    fn caches_and_returns_put_context() {
        let cache = BucketCache::new(None, 256);
        let entry = sample_entry();
        let id = entry.id;
        cache.insert("bench", entry);

        let cached = cache.get("bench").expect("entry");
        assert_eq!(cached.id, id);
    }

    #[test]
    fn updates_versioning_in_place() {
        let cache = BucketCache::new(None, 256);
        cache.insert("b", sample_entry());
        cache.set_versioning("b", true);
        assert!(cache.get("b").unwrap().versioning);
    }
}
