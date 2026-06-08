use std::collections::HashMap;
use std::sync::RwLock;

use crate::iam::Acl;
use crate::storage::CorsRule;
use uuid::Uuid;

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
#[derive(Debug, Default)]
pub struct BucketCache {
    map: RwLock<HashMap<String, CachedBucketEntry>>,
}

impl BucketCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<CachedBucketEntry> {
        self.map.read().ok()?.get(name).cloned()
    }

    pub fn insert(&self, name: impl Into<String>, entry: CachedBucketEntry) {
        if let Ok(mut map) = self.map.write() {
            map.insert(name.into(), entry);
        }
    }

    pub fn remove(&self, name: &str) {
        if let Ok(mut map) = self.map.write() {
            map.remove(name);
        }
    }

    pub fn set_versioning(&self, name: &str, enabled: bool) {
        if let Ok(mut map) = self.map.write() {
            if let Some(entry) = map.get_mut(name) {
                entry.versioning = enabled;
            }
        }
    }

    pub fn set_policy(&self, name: &str, policy: Option<String>) {
        if let Ok(mut map) = self.map.write() {
            if let Some(entry) = map.get_mut(name) {
                entry.policy = policy;
            }
        }
    }

    pub fn set_acl(&self, name: &str, acl: Option<Acl>) {
        if let Ok(mut map) = self.map.write() {
            if let Some(entry) = map.get_mut(name) {
                entry.acl = acl;
            }
        }
    }

    pub fn set_cors(&self, name: &str, rules: Vec<CorsRule>) {
        if let Ok(mut map) = self.map.write() {
            if let Some(entry) = map.get_mut(name) {
                entry.cors_rules = rules;
            }
        }
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
        let cache = BucketCache::new();
        let entry = sample_entry();
        let id = entry.id;
        cache.insert("bench", entry);

        let cached = cache.get("bench").expect("entry");
        assert_eq!(cached.id, id);
    }

    #[test]
    fn updates_versioning_in_place() {
        let cache = BucketCache::new();
        cache.insert("b", sample_entry());
        cache.set_versioning("b", true);
        assert!(cache.get("b").unwrap().versioning);
    }
}
