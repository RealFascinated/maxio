use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::metrics::{MetricsRegistry, cache_name};

/// Cached entry: (date, region, derived_key).
type CacheEntry = (String, String, Vec<u8>);

/// Per-access-key signing key cache. The signing key derived from
/// (secret_key, date, region) only changes once per day, so caching it
/// eliminates 4 sequential HMAC-SHA256 operations on every authenticated request.
pub struct SigningKeyCache {
    /// access_key_id → (date, region, derived_key)
    entries: RwLock<HashMap<String, CacheEntry>>,
    metrics: Option<Arc<MetricsRegistry>>,
}

impl SigningKeyCache {
    pub fn new(metrics: Option<Arc<MetricsRegistry>>) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            metrics,
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
        if let Ok(entries) = self.entries.read() {
            if let Some((cached_date, cached_region, key)) = entries.get(access_key_id) {
                if cached_date == date && cached_region == region {
                    if let Some(m) = &self.metrics {
                        m.record_cache_hit(cache_name::SIGNING_KEY);
                    }
                    return key.clone();
                }
            }
        }
        if let Some(m) = &self.metrics {
            m.record_cache_miss(cache_name::SIGNING_KEY);
        }
        let key = super::signature_v4::derive_signing_key(secret_key, date, region);
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                access_key_id.to_string(),
                (date.to_string(), region.to_string(), key.clone()),
            );
            if let Some(m) = &self.metrics {
                m.set_cache_entries(cache_name::SIGNING_KEY, entries.len());
            }
        }
        key
    }

    /// Removes the entry for an access key (call on key deletion/deactivation).
    pub fn evict(&self, access_key_id: &str) {
        if let Ok(mut entries) = self.entries.write() {
            if entries.remove(access_key_id).is_some() {
                if let Some(m) = &self.metrics {
                    m.record_cache_eviction(cache_name::SIGNING_KEY);
                    m.set_cache_entries(cache_name::SIGNING_KEY, entries.len());
                }
            }
        }
    }
}
