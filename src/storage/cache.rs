use super::StorageError;
use crate::metrics::MetricsRegistry;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::Mutex;

type ObjectKey = (String, String);

pub struct CacheLayer {
    buckets_dir: PathBuf,
    data_buckets_dir: PathBuf,
    max_size: u64,
    writeback: bool,
    flush_interval: Duration,
    lru: Mutex<LruState>,
    dirty: Mutex<HashSet<ObjectKey>>,
    writeback_halted: AtomicBool,
    metrics: Option<Arc<MetricsRegistry>>,
}

struct LruState {
    entries: HashMap<ObjectKey, (Instant, u64)>,
    total_size: u64,
}

impl CacheLayer {
    pub async fn new(
        cache_dir: &str,
        data_buckets_dir: PathBuf,
        max_size: u64,
        writeback: bool,
        flush_interval: Duration,
    ) -> Result<Self, anyhow::Error> {
        let buckets_dir = Path::new(cache_dir).join("buckets");
        fs::create_dir_all(&buckets_dir).await?;
        let layer = Self {
            buckets_dir,
            data_buckets_dir,
            max_size,
            writeback,
            flush_interval,
            lru: Mutex::new(LruState {
                entries: HashMap::new(),
                total_size: 0,
            }),
            dirty: Mutex::new(HashSet::new()),
            writeback_halted: AtomicBool::new(false),
            metrics: None,
        };
        layer.scan_cache_dir().await?;
        layer.sync_gauges().await;
        Ok(layer)
    }

    pub fn with_metrics(mut self, metrics: Arc<MetricsRegistry>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn buckets_dir(&self) -> &Path {
        &self.buckets_dir
    }

    pub fn writeback(&self) -> bool {
        self.writeback
    }

    pub fn is_writeback_halted(&self) -> bool {
        self.writeback_halted.load(Ordering::Relaxed)
    }

    pub fn check_write_allowed(&self) -> Result<(), StorageError> {
        if self.writeback && self.is_writeback_halted() {
            return Err(StorageError::InvalidKey(
                "writeback cache halted: data directory flush backlog".into(),
            ));
        }
        Ok(())
    }

    pub fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir.join(bucket).join(dir).join(".folder")
        } else {
            self.buckets_dir.join(bucket).join(key)
        }
    }

    async fn scan_cache_dir(&self) -> Result<(), anyhow::Error> {
        let mut stack = vec![self.buckets_dir.clone()];
        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(_) => continue,
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let ft = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if ft.is_dir() {
                    if path
                        .file_name()
                        .is_some_and(|n| n == ".uploads" || n == ".versions")
                    {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                if !ft.is_file() {
                    continue;
                }
                let size = fs::metadata(&path).await?.len();
                if let Some((bucket, key)) = self.path_to_object_key(&path) {
                    self.record_access_inner(&bucket, &key, size).await;
                    if self.writeback {
                        let data_path =
                            super::blob::object_path_in(&self.data_buckets_dir, &bucket, &key);
                        let mark_dirty = match fs::metadata(&data_path).await {
                            Ok(meta) => meta.len() != size,
                            Err(_) => true,
                        };
                        if mark_dirty {
                            self.dirty
                                .lock()
                                .await
                                .insert((bucket.clone(), key.clone()));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn path_to_object_key(&self, path: &Path) -> Option<(String, String)> {
        let rel = path.strip_prefix(&self.buckets_dir).ok()?;
        let mut components = rel.components();
        let bucket = components
            .next()?
            .as_os_str()
            .to_string_lossy()
            .into_owned();
        let rest: PathBuf = components.collect();
        if rest.ends_with(".folder") {
            let key = format!("{}/", rest.parent()?.display());
            return Some((bucket, key));
        }
        let key = rest.to_string_lossy().into_owned();
        if key.is_empty() {
            return None;
        }
        Some((bucket, key))
    }

    pub async fn record_read_hit(&self, bucket: &str, key: &str, size: u64) {
        if let Some(m) = &self.metrics {
            m.record_cache_hit();
        }
        self.record_access_inner(bucket, key, size).await;
        self.sync_gauges().await;
    }

    async fn record_access_inner(&self, bucket: &str, key: &str, size: u64) {
        let mut lru = self.lru.lock().await;
        let entry_key = (bucket.to_string(), key.to_string());
        if let Some((_, old_size)) = lru.entries.insert(entry_key, (Instant::now(), size)) {
            lru.total_size = lru.total_size.saturating_sub(old_size);
        }
        lru.total_size += size;
    }

    async fn sync_gauges(&self) {
        let Some(m) = &self.metrics else {
            return;
        };
        let lru = self.lru.lock().await;
        let dirty = self.dirty.lock().await;
        let dirty_bytes: u64 = dirty
            .iter()
            .filter_map(|k| lru.entries.get(k).map(|(_, size)| *size))
            .sum();
        m.set_cache_state(lru.total_size, lru.entries.len(), dirty.len(), dirty_bytes);
        m.set_cache_writeback_halted(self.writeback_halted.load(Ordering::Relaxed));
    }

    pub async fn remove_entry(&self, bucket: &str, key: &str) {
        let mut lru = self.lru.lock().await;
        let entry_key = (bucket.to_string(), key.to_string());
        if let Some((_, size)) = lru.entries.remove(&entry_key) {
            lru.total_size = lru.total_size.saturating_sub(size);
        }
        self.dirty.lock().await.remove(&entry_key);
        self.sync_gauges().await;
    }

    pub async fn mark_dirty(&self, bucket: &str, key: &str, size: u64) {
        self.record_access_inner(bucket, key, size).await;
        self.dirty
            .lock()
            .await
            .insert((bucket.to_string(), key.to_string()));
        self.sync_gauges().await;
    }

    pub async fn mark_clean(&self, bucket: &str, key: &str, size: u64) {
        self.record_access_inner(bucket, key, size).await;
        self.dirty
            .lock()
            .await
            .remove(&(bucket.to_string(), key.to_string()));
        self.sync_gauges().await;
    }

    pub async fn reserve_space(&self, needed: u64) -> Result<Vec<PathBuf>, StorageError> {
        let mut evicted = Vec::new();
        loop {
            let victim = {
                let lru = self.lru.lock().await;
                if lru.total_size + needed <= self.max_size {
                    break;
                }
                let dirty = self.dirty.lock().await;
                lru.entries
                    .iter()
                    .filter(|(k, _)| !dirty.contains(*k))
                    .min_by_key(|(_, (access, _))| *access)
                    .map(|(k, (_, size))| (k.clone(), *size))
            };
            let Some((key, size)) = victim else {
                return Err(StorageError::InvalidKey(
                    "cache full and no clean entries to evict".into(),
                ));
            };
            if let Some(m) = &self.metrics {
                m.record_cache_eviction();
            }
            let path = self.object_path(&key.0, &key.1);
            fs::remove_file(&path).await.map_err(StorageError::Io)?;
            {
                let mut lru = self.lru.lock().await;
                lru.entries.remove(&key);
                lru.total_size = lru.total_size.saturating_sub(size);
            }
            evicted.push(path);
        }
        self.sync_gauges().await;
        Ok(evicted)
    }

    pub async fn populate_from_data(
        &self,
        bucket: &str,
        key: &str,
        data_path: &Path,
        size: u64,
    ) -> Result<PathBuf, StorageError> {
        if let Some(m) = &self.metrics {
            m.record_cache_miss();
        }
        self.reserve_space(size).await?;
        let cache_path = self.object_path(bucket, key);
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).await.map_err(StorageError::Io)?;
        }
        fs::copy(data_path, &cache_path)
            .await
            .map_err(StorageError::Io)?;
        self.record_access_inner(bucket, key, size).await;
        self.sync_gauges().await;
        Ok(cache_path)
    }

    pub async fn flush_dirty(&self) -> Result<(), StorageError> {
        if !self.writeback {
            return Ok(());
        }

        let start = Instant::now();
        let dirty_keys: Vec<ObjectKey> = self.dirty.lock().await.iter().cloned().collect();
        if dirty_keys.is_empty() {
            self.writeback_halted.store(false, Ordering::Relaxed);
            self.sync_gauges().await;
            return Ok(());
        }

        let mut had_failure = false;
        let mut flushed_bytes = 0u64;
        for (bucket, key) in dirty_keys {
            let cache_path = self.object_path(&bucket, &key);
            let data_path = super::blob::object_path_in(&self.data_buckets_dir, &bucket, &key);
            if !fs::try_exists(&cache_path).await.unwrap_or(false) {
                self.dirty
                    .lock()
                    .await
                    .remove(&(bucket.clone(), key.clone()));
                continue;
            }
            let size = fs::metadata(&cache_path)
                .await
                .map_err(StorageError::Io)?
                .len();
            if let Some(parent) = data_path.parent() {
                if let Err(e) = fs::create_dir_all(parent).await {
                    tracing::error!(
                        bucket,
                        key,
                        error = %e,
                        "writeback flush: failed to create data dir"
                    );
                    had_failure = true;
                    continue;
                }
            }
            match fs::copy(&cache_path, &data_path).await {
                Ok(_) => {
                    flushed_bytes += size;
                    self.mark_clean(&bucket, &key, size).await;
                }
                Err(e) => {
                    tracing::error!(
                        bucket,
                        key,
                        error = %e,
                        "writeback flush: failed to copy object to data dir"
                    );
                    had_failure = true;
                }
            }
        }

        let elapsed = start.elapsed();
        if had_failure {
            self.writeback_halted.store(true, Ordering::Relaxed);
            if let Some(m) = &self.metrics {
                m.record_cache_flush(false, flushed_bytes, elapsed);
            }
            self.sync_gauges().await;
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "writeback flush failed",
            )));
        }

        self.writeback_halted.store(false, Ordering::Relaxed);
        if let Some(m) = &self.metrics {
            m.record_cache_flush(true, flushed_bytes, elapsed);
        }
        self.sync_gauges().await;
        Ok(())
    }

    pub fn spawn_flush_task(self: Arc<Self>) {
        if !self.writeback {
            return;
        }
        let interval = self.flush_interval;
        tokio::spawn(async move {
            if let Err(e) = self.flush_dirty().await {
                tracing::warn!("cache writeback startup flush: {}", e);
            }
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if let Err(e) = self.flush_dirty().await {
                    tracing::warn!("cache writeback flush: {}", e);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn scan_and_flush_dirty_after_restart() {
        let cache_root = TempDir::new().unwrap();
        let data_root = TempDir::new().unwrap();
        let data_buckets = data_root.path().join("buckets");
        tokio::fs::create_dir_all(&data_buckets).await.unwrap();

        let cache_path = cache_root
            .path()
            .join("buckets")
            .join("bucket-a")
            .join("obj.txt");
        tokio::fs::create_dir_all(cache_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&cache_path, b"cached payload")
            .await
            .unwrap();

        let layer = CacheLayer::new(
            cache_root.path().to_str().unwrap(),
            data_buckets.clone(),
            1024 * 1024,
            true,
            Duration::from_secs(30),
        )
        .await
        .unwrap();

        let data_path = data_buckets.join("bucket-a").join("obj.txt");
        assert!(
            !tokio::fs::try_exists(&data_path).await.unwrap(),
            "data dir should not have the object before flush"
        );

        layer.flush_dirty().await.unwrap();

        let data = tokio::fs::read(&data_path).await.unwrap();
        assert_eq!(data, b"cached payload");
    }
}
