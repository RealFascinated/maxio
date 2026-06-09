use std::sync::Arc;

use crate::cache::MetricsLruCache;
use crate::metrics::{MetricsRegistry, cache_name};

/// Cached entry: (date, region, derived_key).
type CacheEntry = (String, String, Vec<u8>);

/// Per-access-key signing key cache. The signing key derived from
/// (secret_key, date, region) only changes once per day, so caching it
/// eliminates 4 sequential HMAC-SHA256 operations on every authenticated request.
pub struct SigningKeyCache {
    entries: MetricsLruCache<String, CacheEntry>,
}

impl SigningKeyCache {
    pub fn new(metrics: Option<Arc<MetricsRegistry>>, max_entries: usize) -> Self {
        Self {
            entries: MetricsLruCache::new(metrics, cache_name::SIGNING_KEY, max_entries),
        }
    }

    /// Returns the cached signing key when date and region match; otherwise
    /// derives it, stores it, and returns it.
    pub fn get_or_derive(
        &self,
        access_key_id: &str,
        date: &str,
        region: &str,
        secret_key: &str,
    ) -> Vec<u8> {
        let key = access_key_id.to_string();
        if let Some((cached_date, cached_region, derived)) = self.entries.lookup(&key) {
            if cached_date == date && cached_region == region {
                self.entries.record_hit();
                return derived;
            }
        }
        self.entries.record_miss();
        let derived = super::signature_v4::derive_signing_key(secret_key, date, region);
        self.entries
            .insert(key, (date.to_string(), region.to_string(), derived.clone()));
        derived
    }

    /// Removes the entry for an access key (call on key deletion/deactivation).
    pub fn evict(&self, access_key_id: &str) {
        self.entries.remove(access_key_id);
    }
}
