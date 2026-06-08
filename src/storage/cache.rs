use super::StorageError;
use crate::metrics::MetricsRegistry;
use futures::stream::{self, StreamExt};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::{Mutex, Notify};

type ObjectKey = (String, String);

const INDEX_MAGIC: &[u8; 4] = b"MXIO";
const INDEX_VERSION: u8 = 1;

pub struct CacheLayer {
    cache_dir: PathBuf,
    buckets_dir: PathBuf,
    data_buckets_dir: PathBuf,
    max_size: u64,
    writeback: bool,
    flush_interval: Duration,
    lru: Mutex<LruState>,
    dirty: Mutex<HashSet<ObjectKey>>,
    dirty_bytes: AtomicU64,
    writeback_halted: AtomicBool,
    scan_complete: AtomicBool,
    scan_ready: Notify,
    dirty_scan_complete: AtomicBool,
    dirty_scan_ready: Notify,
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
        let cache_dir = PathBuf::from(cache_dir);
        let buckets_dir = cache_dir.join("buckets");
        fs::create_dir_all(&buckets_dir).await?;
        let layer = Self {
            cache_dir,
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
            dirty_bytes: AtomicU64::new(0),
            writeback_halted: AtomicBool::new(false),
            scan_complete: AtomicBool::new(false),
            scan_ready: Notify::new(),
            dirty_scan_complete: AtomicBool::new(false),
            dirty_scan_ready: Notify::new(),
            metrics: None,
        };
        if let Some((entries, dirty)) = layer.load_index().await? {
            layer.apply_index(entries, dirty).await;
            layer.recalc_dirty_bytes().await;
            layer.scan_complete.store(true, Ordering::Release);
            layer.dirty_scan_complete.store(true, Ordering::Release);
            tracing::info!(
                entries = layer.lru.lock().await.entries.len(),
                "cache: restored LRU index"
            );
        } else if cfg!(test) {
            let found = layer.scan_lru_entries().await?;
            layer.apply_lru_entries(found).await;
            layer.scan_dirty_entries().await;
            layer.recalc_dirty_bytes().await;
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
        self.apply_lru_entries(entries).await;
        *self.dirty.lock().await = dirty;
    }

    pub async fn save_index(&self) -> Result<(), anyhow::Error> {
        let lru = self.lru.lock().await;
        let dirty = self.dirty.lock().await;
        let entries: Vec<(String, String, u64)> = lru
            .entries
            .iter()
            .map(|((bucket, key), (_, size))| (bucket.clone(), key.clone(), *size))
            .collect();
        drop(lru);

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

    /// Walks the cache directory to rebuild LRU state. Runs in the background on startup
    /// unless a persisted index was loaded.
    pub fn spawn_scan_task(self: Arc<Self>) {
        if self.scan_complete.load(Ordering::Acquire) {
            return;
        }
        tokio::spawn(async move {
            let start = Instant::now();
            match self.scan_lru_entries().await {
                Ok(found) => {
                    self.apply_lru_entries(found).await;
                    let entries = self.lru.lock().await.entries.len();
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
                        self.recalc_dirty_bytes().await;
                        let dirty_count = self.dirty.lock().await.len();
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
        });
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
        self.recalc_dirty_bytes().await;
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
        let now = Instant::now();
        let total_size: u64 = found.iter().map(|(_, _, size)| *size).sum();
        let mut lru = self.lru.lock().await;
        lru.entries = found
            .into_iter()
            .map(|(bucket, key, size)| ((bucket, key), (now, size)))
            .collect();
        lru.total_size = total_size;
    }

    async fn scan_dirty_entries(&self) {
        let candidates: Vec<ObjectKey> = {
            let lru = self.lru.lock().await;
            lru.entries.keys().cloned().collect()
        };
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
        *self.dirty.lock().await = dirty;
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

    async fn recalc_dirty_bytes(&self) {
        let lru = self.lru.lock().await;
        let dirty = self.dirty.lock().await;
        let bytes: u64 = dirty
            .iter()
            .filter_map(|k| lru.entries.get(k).map(|(_, size)| *size))
            .sum();
        self.dirty_bytes.store(bytes, Ordering::Relaxed);
    }

    async fn sync_gauges(&self) {
        let Some(m) = &self.metrics else {
            return;
        };
        let lru = self.lru.lock().await;
        let dirty = self.dirty.lock().await;
        m.set_cache_state(
            lru.total_size,
            lru.entries.len(),
            dirty.len(),
            self.dirty_bytes.load(Ordering::Relaxed),
        );
        m.set_cache_writeback_halted(self.writeback_halted.load(Ordering::Relaxed));
    }

    pub async fn remove_entry(&self, bucket: &str, key: &str) {
        let entry_key = (bucket.to_string(), key.to_string());
        let removed_size = {
            let mut lru = self.lru.lock().await;
            lru.entries.remove(&entry_key).map(|(_, size)| {
                lru.total_size = lru.total_size.saturating_sub(size);
                size
            })
        };
        if self.dirty.lock().await.remove(&entry_key) {
            if let Some(size) = removed_size {
                self.dirty_bytes.fetch_sub(size, Ordering::Relaxed);
            }
        }
    }

    pub async fn mark_dirty(&self, bucket: &str, key: &str, size: u64) {
        let entry_key = (bucket.to_string(), key.to_string());
        let old_size = {
            let lru = self.lru.lock().await;
            lru.entries.get(&entry_key).map(|(_, s)| *s)
        };
        self.record_access_inner(bucket, key, size).await;
        let mut dirty = self.dirty.lock().await;
        if dirty.insert(entry_key) {
            self.dirty_bytes.fetch_add(size, Ordering::Relaxed);
        } else if let Some(old) = old_size {
            if size != old {
                self.dirty_bytes
                    .fetch_add(size.saturating_sub(old), Ordering::Relaxed);
            }
        }
    }

    pub async fn mark_clean(&self, bucket: &str, key: &str, size: u64) {
        let entry_key = (bucket.to_string(), key.to_string());
        self.record_access_inner(bucket, key, size).await;
        let mut dirty = self.dirty.lock().await;
        if dirty.remove(&entry_key) {
            let bytes = {
                let lru = self.lru.lock().await;
                lru.entries.get(&entry_key).map(|(_, s)| *s).unwrap_or(size)
            };
            self.dirty_bytes.fetch_sub(bytes, Ordering::Relaxed);
        }
    }

    pub async fn reserve_space(&self, needed: u64) -> Result<Vec<PathBuf>, StorageError> {
        self.wait_until_scan_complete().await;
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
        Ok(cache_path)
    }

    pub async fn flush_dirty(&self) -> Result<(), StorageError> {
        if !self.writeback {
            return Ok(());
        }
        self.wait_until_scan_complete().await;
        self.wait_until_dirty_scan_complete().await;

        let start = Instant::now();
        let dirty_keys: Vec<ObjectKey> = self.dirty.lock().await.iter().cloned().collect();
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
                    self.dirty
                        .lock()
                        .await
                        .remove(&(bucket.clone(), key.clone()));
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
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "writeback flush failed",
            )));
        }

        self.writeback_halted.store(false, Ordering::Relaxed);
        if let Some(m) = &self.metrics {
            m.record_cache_flush(true, flushed_bytes, elapsed);
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

fn encode_index(
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

fn decode_index(data: &[u8]) -> io::Result<(Vec<(String, String, u64)>, HashSet<ObjectKey>)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn index_roundtrip() {
        let entries = vec![
            ("bucket-a".into(), "path/obj.txt".into(), 42u64),
            ("bucket-b".into(), "folder/".into(), 0u64),
        ];
        let mut dirty = HashSet::new();
        dirty.insert(("bucket-a".into(), "path/obj.txt".into()));
        let data = encode_index(&entries, &dirty).unwrap();
        let (decoded_entries, decoded_dirty) = decode_index(&data).unwrap();
        assert_eq!(decoded_entries, entries);
        assert_eq!(decoded_dirty, dirty);
    }

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
