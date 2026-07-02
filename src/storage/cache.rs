use super::StorageError;
use super::disk_cache_state::{CacheStateHandle, DiskCacheState, ObjectKey};
use crate::metrics::{MetricsRegistry, cache_name};
use futures::stream::{self, StreamExt};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::Notify;

const INDEX_MAGIC: &[u8; 4] = b"MXIO";
const INDEX_VERSION: u8 = 1;

pub struct CacheLayer {
    cache_dir: PathBuf,
    buckets_dir: PathBuf,
    data_buckets_dir: PathBuf,
    max_size: u64,
    writeback: bool,
    flush_interval: Duration,
    state: Arc<DiskCacheState>,
    state_handle: CacheStateHandle,
    writeback_halted: AtomicBool,
    scan_complete: AtomicBool,
    scan_ready: Notify,
    dirty_scan_complete: AtomicBool,
    dirty_scan_ready: Notify,
    /// Set when LRU was seeded from .lru-index.bin; tells spawn_scan_task to run
    /// a merge scan (discover/drop) rather than a full replace scan.
    index_loaded: AtomicBool,
    /// Merge scan finished (index-loaded restarts only). Writers use `scan_complete`.
    merge_scan_complete: AtomicBool,
    metrics: Option<Arc<MetricsRegistry>>,
}

impl CacheLayer {
    pub async fn new(
        cache_dir: &str,
        data_buckets_dir: PathBuf,
        max_size: u64,
        writeback: bool,
        flush_interval: Duration,
    ) -> Result<Self, anyhow::Error> {
        let cache_dir = PathBuf::from(cache_dir);
        let buckets_dir = cache_dir.join("buckets");
        fs::create_dir_all(&buckets_dir).await?;
        let state = Arc::new(DiskCacheState::new());
        let state_handle = CacheStateHandle::spawn(Arc::clone(&state));
        let layer = Self {
            cache_dir,
            buckets_dir,
            data_buckets_dir,
            max_size,
            writeback,
            flush_interval,
            state,
            state_handle,
            writeback_halted: AtomicBool::new(false),
            scan_complete: AtomicBool::new(false),
            scan_ready: Notify::new(),
            dirty_scan_complete: AtomicBool::new(false),
            dirty_scan_ready: Notify::new(),
            index_loaded: AtomicBool::new(false),
            merge_scan_complete: AtomicBool::new(false),
            metrics: None,
        };
        if let Some((entries, dirty)) = layer.load_index().await? {
            // Trust the on-disk index for eviction sizing; reconcile in the background so
            // PUTs are not blocked for minutes on large caches (596k+ entries).
            layer.apply_index(entries, dirty).await;
            layer.index_loaded.store(true, Ordering::Release);
            layer.scan_complete.store(true, Ordering::Release);
            layer.scan_ready.notify_waiters();
            tracing::info!(
                entries = layer.state.entry_count(),
                "cache: loaded LRU index, reconciling with filesystem"
            );
        } else if cfg!(test) {
            let found = layer.scan_lru_entries().await?;
            layer.apply_lru_entries(found).await;
            layer.scan_dirty_entries().await;
            layer.scan_complete.store(true, Ordering::Release);
            layer.dirty_scan_complete.store(true, Ordering::Release);
        }
        Ok(layer)
    }

    fn index_path(&self) -> PathBuf {
        self.cache_dir.join(".lru-index.bin")
    }

    async fn load_index(
        &self,
    ) -> Result<Option<(Vec<(String, String, u64)>, HashSet<ObjectKey>)>, anyhow::Error> {
        let path = self.index_path();
        let data = match fs::read(&path).await {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        match decode_index(&data) {
            Ok(index) => Ok(Some(index)),
            Err(e) => {
                tracing::warn!("cache: ignoring corrupt index at {}: {e}", path.display());
                Ok(None)
            }
        }
    }

    async fn apply_index(&self, entries: Vec<(String, String, u64)>, dirty: HashSet<ObjectKey>) {
        self.state.apply_bulk(&entries, &dirty);
    }

    pub async fn save_index(&self) -> Result<(), anyhow::Error> {
        self.state_handle.drain().await;
        let entries = self.state.all_entries();
        let dirty = self.state.all_dirty();

        let path = self.index_path();
        let tmp = path.with_extension("bin.tmp");
        let data = encode_index(&entries, &dirty)?;
        fs::write(&tmp, data).await?;
        fs::rename(&tmp, &path).await?;
        Ok(())
    }

    async fn wait_until_scan_complete(&self) {
        if self.scan_complete.load(Ordering::Acquire) {
            return;
        }
        self.scan_ready.notified().await;
    }

    async fn wait_until_dirty_scan_complete(&self) {
        if self.dirty_scan_complete.load(Ordering::Acquire) {
            return;
        }
        self.dirty_scan_ready.notified().await;
    }

    /// Walks the cache directory to rebuild or reconcile LRU state.
    ///
    /// - No index: full replace scan. Replaces LRU entirely from filesystem.
    /// - Index loaded: merge scan. Adds unindexed files, drops stale entries.
    ///
    /// In both cases sets `scan_complete` when done so pending operations can proceed.
    pub fn spawn_scan_task(self: Arc<Self>) {
        if self.index_loaded.load(Ordering::Acquire) {
            if self.merge_scan_complete.load(Ordering::Acquire) {
                return;
            }
        } else if self.scan_complete.load(Ordering::Acquire) {
            return;
        }
        tokio::spawn(async move {
            if self.index_loaded.load(Ordering::Acquire) {
                self.run_merge_scan().await;
            } else {
                self.run_fresh_scan().await;
            }
        });
    }

    async fn run_fresh_scan(&self) {
        let start = Instant::now();
        match self.scan_lru_entries().await {
            Ok(found) => {
                self.apply_lru_entries(found).await;
                let entries = self.state.entry_count();
                self.scan_complete.store(true, Ordering::Release);
                self.scan_ready.notify_waiters();
                tracing::info!(
                    entries,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "cache: directory scan complete"
                );
                if let Err(e) = self.save_index().await {
                    tracing::warn!("cache index save after scan: {e}");
                }
                if self.writeback {
                    let dirty_start = Instant::now();
                    self.scan_dirty_entries().await;
                    let dirty_count = self.state.dirty_count();
                    self.dirty_scan_complete.store(true, Ordering::Release);
                    self.dirty_scan_ready.notify_waiters();
                    tracing::debug!(
                        dirty = dirty_count,
                        elapsed_ms = dirty_start.elapsed().as_millis() as u64,
                        "cache: dirty scan complete"
                    );
                    if let Err(e) = self.save_index().await {
                        tracing::warn!("cache index save after dirty scan: {e}");
                    }
                } else {
                    self.dirty_scan_complete.store(true, Ordering::Release);
                    self.dirty_scan_ready.notify_waiters();
                }
            }
            Err(e) => {
                tracing::error!("cache directory scan failed: {e}");
                self.scan_complete.store(true, Ordering::Release);
                self.scan_ready.notify_waiters();
                self.dirty_scan_complete.store(true, Ordering::Release);
                self.dirty_scan_ready.notify_waiters();
            }
        }
    }

    /// Reconciles the index-seeded LRU against the actual cache filesystem.
    /// Adds unindexed files, drops phantom entries, then sets scan_complete so
    /// reserve_space and the trimmer see accurate total_size before running.
    async fn run_merge_scan(&self) {
        let start = Instant::now();
        // Snapshot LRU keys before the filesystem walk. Entries evicted by
        // concurrent reserve_space calls (shouldn't happen yet since scan_complete
        // is still false at this point) would otherwise be re-added as phantoms.
        let pre_scan_keys = self.state.all_keys();
        let found = match self.scan_lru_entries().await {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("cache: merge scan failed: {e}");
                self.merge_scan_complete.store(true, Ordering::Release);
                self.dirty_scan_complete.store(true, Ordering::Release);
                self.dirty_scan_ready.notify_waiters();
                return;
            }
        };
        let on_disk: HashMap<ObjectKey, u64> =
            found.into_iter().map(|(b, k, s)| ((b, k), s)).collect();
        let (removed, added) = self.state.merge_reconcile(&pre_scan_keys, &on_disk);
        let entries = self.state.entry_count();
        tracing::info!(
            removed,
            added,
            entries,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "cache: merged index with filesystem"
        );
        if let Err(e) = self.save_index().await {
            tracing::warn!("cache: index save after merge: {e}");
        }
        self.merge_scan_complete.store(true, Ordering::Release);
        if self.writeback {
            self.scan_dirty_entries().await;
        }
        self.dirty_scan_complete.store(true, Ordering::Release);
        self.dirty_scan_ready.notify_waiters();
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
        self.state_handle.drain().await;
        self.state.purge_bucket_sync(bucket);
    }

    pub fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir.join(bucket).join(dir).join(".folder")
        } else {
            self.buckets_dir.join(bucket).join(key)
        }
    }

    async fn scan_lru_entries(&self) -> Result<Vec<(String, String, u64)>, anyhow::Error> {
        let buckets_dir = self.buckets_dir.clone();
        tokio::task::spawn_blocking(move || scan_buckets_sync(&buckets_dir))
            .await
            .map_err(|e| anyhow::anyhow!("cache scan task failed: {e}"))?
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn apply_lru_entries(&self, found: Vec<(String, String, u64)>) {
        self.state.apply_bulk(&found, &HashSet::new());
    }

    async fn scan_dirty_entries(&self) {
        let candidates: Vec<ObjectKey> = self.state.all_keys().into_iter().collect();
        let data_buckets_dir = self.data_buckets_dir.clone();
        let dirty: HashSet<ObjectKey> = stream::iter(candidates)
            .map(|(bucket, key)| {
                let data_buckets_dir = data_buckets_dir.clone();
                async move {
                    let data_path = super::blob::object_path_in(&data_buckets_dir, &bucket, &key);
                    if fs::try_exists(&data_path).await.unwrap_or(false) {
                        None
                    } else {
                        Some((bucket, key))
                    }
                }
            })
            .buffer_unordered(512)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;
        self.state.set_dirty_set(dirty);
    }

    pub async fn record_read_hit(&self, bucket: &str, key: &str, size: u64) {
        if let Some(m) = &self.metrics {
            m.record_cache_hit(cache_name::OBJECT_DISK);
        }
        self.state_handle.record_hit(bucket, key, size);
    }

    async fn sync_gauges(&self) {
        let Some(m) = &self.metrics else {
            return;
        };
        m.set_cache_state(
            cache_name::OBJECT_DISK,
            self.state.total_size(),
            self.state.entry_count(),
            self.state.dirty_count(),
            self.state.dirty_bytes(),
        );
        m.set_cache_writeback_halted(
            cache_name::OBJECT_DISK,
            self.writeback_halted.load(Ordering::Relaxed),
        );
    }

    pub async fn remove_entry(&self, bucket: &str, key: &str) {
        self.state_handle.remove(bucket, key).await;
    }

    pub async fn mark_dirty(&self, bucket: &str, key: &str, size: u64) {
        self.state_handle.mark_dirty(bucket, key, size);
    }

    pub async fn mark_clean(&self, bucket: &str, key: &str, size: u64) {
        self.state_handle.drain().await;
        self.state.mark_clean_sync(bucket, key, size);
    }

    pub async fn reserve_space(&self, needed: u64) -> Result<Vec<PathBuf>, StorageError> {
        self.wait_until_scan_complete().await;
        self.state_handle.drain().await;
        let mut evicted = Vec::new();
        loop {
            if self.state.total_size() + needed <= self.max_size {
                break;
            }
            let Some((key, size)) = self.state.pop_clean_lru() else {
                return Err(StorageError::InvalidKey(
                    "cache full and no clean entries to evict".into(),
                ));
            };
            if let Some(m) = &self.metrics {
                m.record_cache_eviction(cache_name::OBJECT_DISK);
            }
            let path = self.object_path(&key.0, &key.1);
            match fs::remove_file(&path).await {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(StorageError::Io(e)),
            }
            let _ = size;
            evicted.push(path);
        }
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
            m.record_cache_miss(cache_name::OBJECT_DISK);
        }
        self.reserve_space(size).await?;
        let cache_path = self.object_path(bucket, key);
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).await.map_err(StorageError::Io)?;
        }
        fs::copy(data_path, &cache_path)
            .await
            .map_err(StorageError::Io)?;
        if let Some(m) = &self.metrics {
            m.record_drive_read_op();
        }
        self.state_handle.record_hit(bucket, key, size);
        Ok(cache_path)
    }

    pub async fn flush_dirty(&self) -> Result<(), StorageError> {
        if !self.writeback {
            return Ok(());
        }
        self.wait_until_scan_complete().await;
        self.wait_until_dirty_scan_complete().await;
        self.state_handle.drain().await;

        let start = Instant::now();
        let dirty_keys: Vec<ObjectKey> = self.state.all_dirty().into_iter().collect();
        if dirty_keys.is_empty() {
            self.writeback_halted.store(false, Ordering::Relaxed);
            return Ok(());
        }

        let mut had_failure = false;
        let mut flushed_bytes = 0u64;
        for (bucket, key) in dirty_keys {
            let cache_path = self.object_path(&bucket, &key);
            let data_path = super::blob::object_path_in(&self.data_buckets_dir, &bucket, &key);
            let size = match fs::metadata(&cache_path).await {
                Ok(m) => m.len(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    self.state.remove_sync(&bucket, &key);
                    continue;
                }
                Err(e) => return Err(StorageError::Io(e)),
            };
            if fs::try_exists(&data_path).await.unwrap_or(false) {
                let data_size = fs::metadata(&data_path)
                    .await
                    .map_err(StorageError::Io)?
                    .len();
                if data_size == size {
                    self.mark_clean(&bucket, &key, size).await;
                    continue;
                }
            }
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
                    if let Some(m) = &self.metrics {
                        m.record_drive_write_op();
                    }
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
                m.record_cache_flush(cache_name::OBJECT_DISK, false, flushed_bytes, elapsed);
            }
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "writeback flush failed",
            )));
        }

        self.writeback_halted.store(false, Ordering::Relaxed);
        if let Some(m) = &self.metrics {
            m.record_cache_flush(cache_name::OBJECT_DISK, true, flushed_bytes, elapsed);
        }
        Ok(())
    }

    /// Publishes cache gauge metrics on a fixed interval so hot paths never scan state.
    pub fn spawn_gauge_task(self: Arc<Self>) {
        if self.metrics.is_none() {
            return;
        }
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            self.sync_gauges().await;
            loop {
                ticker.tick().await;
                self.sync_gauges().await;
            }
        });
    }

    /// Continuously evicts LRU clean entries when `total_size` exceeds `max_size`.
    pub fn spawn_eviction_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if self.state.total_size() <= self.max_size {
                    continue;
                }
                let size_before = self.state.total_size();
                tracing::info!(
                    size_gb = size_before as f64 / 1e9,
                    max_gb = self.max_size as f64 / 1e9,
                    "cache: over limit, evicting in background"
                );
                let mut evicted = 0u64;
                while self.state.total_size() > self.max_size {
                    match self.evict_one_clean().await {
                        Ok(true) => {
                            evicted += 1;
                            if evicted.is_multiple_of(64) {
                                tokio::task::yield_now().await;
                            }
                        }
                        Ok(false) => break,
                        Err(e) => {
                            tracing::warn!("cache eviction failed: {e}");
                            break;
                        }
                    }
                }
                if evicted > 0 {
                    let size_after = self.state.total_size();
                    tracing::info!(
                        evicted,
                        freed_gb = size_before.saturating_sub(size_after) as f64 / 1e9,
                        size_gb = size_after as f64 / 1e9,
                        "cache: eviction complete"
                    );
                }
            }
        });
    }

    async fn evict_one_clean(&self) -> Result<bool, StorageError> {
        self.state_handle.drain().await;
        let Some((key, _size)) = self.state.pop_clean_lru() else {
            return Ok(false);
        };
        if let Some(m) = &self.metrics {
            m.record_cache_eviction(cache_name::OBJECT_DISK);
        }
        let path = self.object_path(&key.0, &key.1);
        match fs::remove_file(&path).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    /// Periodically persists the LRU index so restarts re-discover fewer files.
    pub fn spawn_index_save_task(self: Arc<Self>) {
        tokio::spawn(async move {
            self.wait_until_scan_complete().await;
            let mut ticker = tokio::time::interval(Duration::from_secs(5 * 60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await; // skip the immediate first tick
            loop {
                ticker.tick().await;
                if let Err(e) = self.save_index().await {
                    tracing::warn!("cache: periodic index save failed: {e}");
                }
            }
        });
    }

    pub fn spawn_flush_task(self: Arc<Self>) {
        if !self.writeback {
            return;
        }
        let interval = self.flush_interval;
        tokio::spawn(async move {
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

fn path_to_object_key(buckets_dir: &Path, path: &Path) -> Option<(String, String)> {
    let rel = path.strip_prefix(buckets_dir).ok()?;
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

fn scan_buckets_sync(buckets_dir: &Path) -> io::Result<Vec<(String, String, u64)>> {
    let bucket_dirs: Vec<PathBuf> = match std::fs::read_dir(buckets_dir) {
        Ok(read_dir) => read_dir
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
            .map(|entry| entry.path())
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    if bucket_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        for bucket_dir in bucket_dirs {
            let tx = tx.clone();
            let buckets_dir = buckets_dir.to_path_buf();
            scope.spawn(move || {
                let mut found = Vec::new();
                if walk_bucket_dir(&buckets_dir, &bucket_dir, &mut found).is_ok() {
                    let _ = tx.send(found);
                }
            });
        }
        drop(tx);
    });

    let mut found = Vec::new();
    for chunk in rx {
        found.extend(chunk);
    }
    Ok(found)
}

fn walk_bucket_dir(
    buckets_dir: &Path,
    dir: &Path,
    found: &mut Vec<(String, String, u64)>,
) -> io::Result<()> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = entry.metadata()?;
            if meta.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|n| n == ".uploads" || n == ".versions")
                {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            if let Some((bucket, key)) = path_to_object_key(buckets_dir, &path) {
                found.push((bucket, key, meta.len()));
            }
        }
    }
    Ok(())
}

pub fn encode_index(
    entries: &[(String, String, u64)],
    dirty: &HashSet<ObjectKey>,
) -> io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(entries.len() * 48 + dirty.len() * 32 + 32);
    buf.extend_from_slice(INDEX_MAGIC);
    buf.push(INDEX_VERSION);
    buf.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for (bucket, key, size) in entries {
        write_string(&mut buf, bucket.as_bytes(), u16::MAX as usize)?;
        write_string(&mut buf, key.as_bytes(), u32::MAX as usize)?;
        buf.extend_from_slice(&size.to_le_bytes());
    }
    buf.extend_from_slice(&(dirty.len() as u64).to_le_bytes());
    for (bucket, key) in dirty {
        write_string(&mut buf, bucket.as_bytes(), u16::MAX as usize)?;
        write_string(&mut buf, key.as_bytes(), u32::MAX as usize)?;
    }
    Ok(buf)
}

fn write_string(buf: &mut Vec<u8>, value: &[u8], max_len: usize) -> io::Result<()> {
    if value.len() > max_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache index string too long",
        ));
    }
    let len = value.len();
    if max_len <= u16::MAX as usize {
        buf.extend_from_slice(&(len as u16).to_le_bytes());
    } else {
        buf.extend_from_slice(&(len as u32).to_le_bytes());
    }
    buf.extend_from_slice(value);
    Ok(())
}

type IndexEntries = (Vec<(String, String, u64)>, HashSet<ObjectKey>);

pub fn decode_index(data: &[u8]) -> io::Result<IndexEntries> {
    let mut offset = 0;
    let magic = read_bytes(data, &mut offset, 4)?;
    if magic != INDEX_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache index has invalid magic",
        ));
    }
    let version = read_u8(data, &mut offset)?;
    if version != INDEX_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache index has unsupported version",
        ));
    }

    let entry_count = read_u64(data, &mut offset)? as usize;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let bucket = read_string(data, &mut offset, u16::MAX as usize)?;
        let key = read_string(data, &mut offset, u32::MAX as usize)?;
        let size = read_u64(data, &mut offset)?;
        entries.push((bucket, key, size));
    }

    let dirty_count = read_u64(data, &mut offset)? as usize;
    let mut dirty = HashSet::with_capacity(dirty_count);
    for _ in 0..dirty_count {
        let bucket = read_string(data, &mut offset, u16::MAX as usize)?;
        let key = read_string(data, &mut offset, u32::MAX as usize)?;
        dirty.insert((bucket, key));
    }

    if offset != data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache index has trailing bytes",
        ));
    }
    Ok((entries, dirty))
}

fn read_bytes<'a>(data: &'a [u8], offset: &mut usize, len: usize) -> io::Result<&'a [u8]> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cache index overflow"))?;
    if end > data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache index truncated",
        ));
    }
    let slice = &data[*offset..end];
    *offset = end;
    Ok(slice)
}

fn read_u8(data: &[u8], offset: &mut usize) -> io::Result<u8> {
    Ok(read_bytes(data, offset, 1)?[0])
}

fn read_u64(data: &[u8], offset: &mut usize) -> io::Result<u64> {
    let bytes = read_bytes(data, offset, 8)?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_string(data: &[u8], offset: &mut usize, max_len: usize) -> io::Result<String> {
    let len = if max_len <= u16::MAX as usize {
        let bytes = read_bytes(data, offset, 2)?;
        u16::from_le_bytes(bytes.try_into().unwrap()) as usize
    } else {
        let bytes = read_bytes(data, offset, 4)?;
        u32::from_le_bytes(bytes.try_into().unwrap()) as usize
    };
    if len > max_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache index string length out of range",
        ));
    }
    let value = read_bytes(data, offset, len)?;
    String::from_utf8(value.to_vec()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cache index string is not valid UTF-8",
        )
    })
}
