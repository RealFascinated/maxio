use std::borrow::Borrow;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};

use lru::LruCache;

use crate::metrics::MetricsRegistry;

fn capacity(max_entries: usize) -> NonZeroUsize {
    NonZeroUsize::new(max_entries.max(1)).expect("non-zero capacity")
}

/// Bounded LRU cache with optional hit/miss/eviction metrics.
pub struct MetricsLruCache<K, V> {
    inner: RwLock<LruCache<K, V>>,
    metrics: Option<Arc<MetricsRegistry>>,
    name: &'static str,
}

impl<K, V> MetricsLruCache<K, V>
where
    K: Hash + Eq + Clone,
{
    pub fn new(
        metrics: Option<Arc<MetricsRegistry>>,
        name: &'static str,
        max_entries: usize,
    ) -> Self {
        Self {
            inner: RwLock::new(LruCache::new(capacity(max_entries))),
            metrics,
            name,
        }
    }

    pub fn record_hit(&self) {
        if let Some(m) = &self.metrics {
            m.record_cache_hit(self.name);
        }
    }

    pub fn record_miss(&self) {
        if let Some(m) = &self.metrics {
            m.record_cache_miss(self.name);
        }
    }

    /// Promotes the entry and records a hit when present.
    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
        V: Clone,
    {
        let value = self.inner.write().ok()?.get(key).cloned();
        if value.is_some() {
            self.record_hit();
        }
        value
    }

    /// Promotes the entry without recording metrics.
    pub fn lookup<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
        V: Clone,
    {
        self.inner.write().ok()?.get(key).cloned()
    }

    /// Returns a clone when `valid` passes; records a hit. Discards invalid entries
    /// without recording an eviction (e.g. TTL expiry).
    pub fn get_if<Q>(&self, key: &Q, valid: impl FnOnce(&V) -> bool) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
        V: Clone,
    {
        if let Some(value) = self.lookup(key) {
            if valid(&value) {
                self.record_hit();
                return Some(value);
            }
            self.discard(key);
        }
        None
    }

    pub fn get_mut<Q, R>(&self, key: &Q, f: impl FnOnce(&mut V) -> R) -> Option<R>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut guard = self.inner.write().ok()?;
        let entry = guard.get_mut(key)?;
        Some(f(entry))
    }

    pub fn insert(&self, key: K, value: V) {
        if let Ok(mut map) = self.inner.write() {
            if map.push(key, value).is_some() {
                if let Some(m) = &self.metrics {
                    m.record_cache_eviction(self.name);
                }
            }
            self.sync_entries(map.len());
        }
    }

    /// Removes an entry (invalidation; does not count as an LRU eviction).
    pub fn remove<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Ok(mut map) = self.inner.write() {
            if map.pop(key).is_some() {
                self.sync_entries(map.len());
                return true;
            }
        }
        false
    }

    /// Drops an entry without recording an eviction (e.g. TTL expiry).
    pub fn discard<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Ok(mut map) = self.inner.write() {
            if map.pop(key).is_some() {
                self.sync_entries(map.len());
                return true;
            }
        }
        false
    }

    pub fn remove_many(&self, keys: &[K]) -> u64 {
        if keys.is_empty() {
            return 0;
        }
        if let Ok(mut map) = self.inner.write() {
            let mut removed = 0u64;
            for key in keys {
                if map.pop(key).is_some() {
                    removed += 1;
                }
            }
            if removed > 0 {
                self.sync_entries(map.len());
            }
            return removed;
        }
        0
    }

    pub fn remove_where<F>(&self, mut pred: F) -> u64
    where
        F: FnMut(&K) -> bool,
    {
        if let Ok(mut map) = self.inner.write() {
            let keys: Vec<K> = map
                .iter()
                .filter(|(k, _)| pred(k))
                .map(|(k, _)| k.clone())
                .collect();
            let removed = keys.len() as u64;
            for key in keys {
                map.pop(&key);
            }
            if removed > 0 {
                self.sync_entries(map.len());
            }
            return removed;
        }
        0
    }

    fn sync_entries(&self, entries: usize) {
        if let Some(m) = &self.metrics {
            m.set_cache_entries(self.name, entries);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_lru_when_at_capacity() {
        let cache = MetricsLruCache::<&str, &str>::new(None, "test", 2);
        cache.insert("first", "1");
        cache.insert("second", "2");
        cache.get(&"first");
        cache.insert("third", "3");

        assert_eq!(cache.get(&"first"), Some("1"));
        assert_eq!(cache.get(&"second"), None);
        assert_eq!(cache.get(&"third"), Some("3"));
    }
}
