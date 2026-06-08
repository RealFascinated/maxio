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

/// Max time to wait for a single cache→array copy before trying another dirty object.
const FLUSH_COPY_TIMEOUT: Duration = Duration::from_secs(120);

pub struct CacheLayer {
    buckets_dir: PathBuf,
    data_buckets_dir: PathBuf,
    max_size: u64,
    writeback: bool,
    flush_interval: Duration,
    lru: Mutex<LruState>,
    dirty: Mutex<HashSet<ObjectKey>>,
    /// Last object whose flush failed; skipped on the next attempt when alternatives exist.
    flush_skip: Mutex<Option<ObjectKey>>,
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
            flush_skip: Mutex::new(None),
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

    pub async fn purge_bucket(&self, bucket: &str) {
        let mut lru = self.lru.lock().await;
        lru.entries.retain(|(b, _), _| b != bucket);
        lru.total_size = lru.entries.values().map(|(_, size)| *size).sum();
        drop(lru);
        self.dirty.lock().await.retain(|(b, _)| b != bucket);
        *self.flush_skip.lock().await = None;
        self.sync_gauges().await;
    }

    async fn pick_dirty_key(&self) -> Option<ObjectKey> {
        let dirty = self.dirty.lock().await;
        if dirty.is_empty() {
            return None;
        }
        let skip = self.flush_skip.lock().await.clone();
        if let Some(ref skip_key) = skip {
            if let Some(key) = dirty.iter().find(|k| *k != skip_key) {
                return Some(key.clone());
            }
        }
        dirty.iter().next().cloned()
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
                    if self.writeback
                        && !fs::try_exists(&super::blob::object_path_in(
                            &self.data_buckets_dir,
                            &bucket,
                            &key,
                        ))
                        .await
                        .unwrap_or(false)
                    {
                        self.dirty
                            .lock()
                            .await
                            .insert((bucket.clone(), key.clone()));
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

    pub async fn is_dirty(&self, bucket: &str, key: &str) -> bool {
        self.dirty
            .lock()
            .await
            .contains(&(bucket.to_string(), key.to_string()))
    }

    pub async fn record_read_hit(&self, bucket: &str, key: &str, size: u64) {
        if let Some(m) = &self.metrics {
            m.record_cache_hit();
        }
        self.record_access_inner(bucket, key, size).await;
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
        let entry_key = (bucket.to_string(), key.to_string());
        {
            let mut lru = self.lru.lock().await;
            if let Some((_, size)) = lru.entries.remove(&entry_key) {
                lru.total_size = lru.total_size.saturating_sub(size);
            }
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

    /// Flush one dirty object to the data directory.
    ///
    /// Returns `Ok(true)` when an entry was processed, `Ok(false)` when the dirty set is empty.
    /// Copies run on a dedicated thread so large array writes do not starve the Tokio blocking
    /// pool used by request-path I/O.
    pub async fn flush_one_dirty(&self) -> Result<bool, StorageError> {
        if !self.writeback {
            return Ok(false);
        }

        let Some((bucket, key)) = self.pick_dirty_key().await else {
            self.writeback_halted.store(false, Ordering::Relaxed);
            *self.flush_skip.lock().await = None;
            self.sync_gauges().await;
            return Ok(false);
        };

        let start = Instant::now();
        let cache_path = self.object_path(&bucket, &key);
        let data_path = super::blob::object_path_in(&self.data_buckets_dir, &bucket, &key);

        if !fs::try_exists(&cache_path).await.unwrap_or(false) {
            self.dirty
                .lock()
                .await
                .remove(&(bucket.clone(), key.clone()));
            self.sync_gauges().await;
            return Ok(true);
        }

        let size = fs::metadata(&cache_path)
            .await
            .map_err(StorageError::Io)?
            .len();

        if fs::try_exists(&data_path).await.unwrap_or(false) {
            let data_size = fs::metadata(&data_path)
                .await
                .map_err(StorageError::Io)?
                .len();
            if data_size == size {
                self.writeback_halted.store(false, Ordering::Relaxed);
                *self.flush_skip.lock().await = None;
                self.mark_clean(&bucket, &key, size).await;
                return Ok(true);
            }
        }

        match copy_on_flush_thread(cache_path.clone(), data_path.clone()).await {
            Ok(_) => {
                self.writeback_halted.store(false, Ordering::Relaxed);
                *self.flush_skip.lock().await = None;
                self.mark_clean(&bucket, &key, size).await;
                if let Some(m) = &self.metrics {
                    m.record_cache_flush(true, size, start.elapsed());
                }
                Ok(true)
            }
            Err(e) => {
                tracing::error!(
                    bucket,
                    key,
                    cache = %cache_path.display(),
                    data = %data_path.display(),
                    error = %e,
                    "writeback flush: failed to copy object to data dir"
                );
                self.writeback_halted.store(true, Ordering::Relaxed);
                *self.flush_skip.lock().await = Some((bucket, key));
                if let Some(m) = &self.metrics {
                    m.record_cache_flush(false, 0, start.elapsed());
                }
                self.sync_gauges().await;
                Err(StorageError::Io(e))
            }
        }
    }

    pub async fn flush_dirty(&self) -> Result<(), StorageError> {
        while self.flush_one_dirty().await? {}
        Ok(())
    }

    pub fn spawn_flush_task(self: Arc<Self>) {
        if !self.writeback {
            return;
        }
        let idle_interval = self.flush_interval;
        tokio::spawn(async move {
            let mut idle_ticker = tokio::time::interval(idle_interval);
            idle_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                match self.flush_one_dirty().await {
                    Ok(true) => {
                        // Yield between objects so request-path array I/O is not starved.
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Ok(false) => {
                        idle_ticker.tick().await;
                    }
                    Err(e) => {
                        tracing::warn!("cache writeback flush: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
}

async fn copy_on_flush_thread(
    cache_path: PathBuf,
    data_path: PathBuf,
) -> Result<u64, std::io::Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = (|| {
            if let Some(parent) = data_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&cache_path, &data_path)
        })();
        let _ = tx.send(result);
    });
    match tokio::time::timeout(FLUSH_COPY_TIMEOUT, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "flush copy thread dropped",
        )),
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "flush copy timed out after {}s",
                FLUSH_COPY_TIMEOUT.as_secs()
            ),
        )),
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

    #[tokio::test]
    async fn flush_skips_copy_when_data_already_matches() {
        let cache_root = TempDir::new().unwrap();
        let data_root = TempDir::new().unwrap();
        let data_buckets = data_root.path().join("buckets");
        let data_path = data_buckets.join("bucket-a").join("obj.txt");
        let cache_path = cache_root
            .path()
            .join("buckets")
            .join("bucket-a")
            .join("obj.txt");
        tokio::fs::create_dir_all(cache_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::create_dir_all(data_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&cache_path, b"same bytes").await.unwrap();
        tokio::fs::write(&data_path, b"same bytes").await.unwrap();

        let layer = CacheLayer::new(
            cache_root.path().to_str().unwrap(),
            data_buckets,
            1024 * 1024,
            true,
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        layer.mark_dirty("bucket-a", "obj.txt", 10).await;

        layer.flush_one_dirty().await.unwrap();

        assert!(
            !layer.is_dirty("bucket-a", "obj.txt").await,
            "matching data file should clear dirty without copying"
        );
    }
}
