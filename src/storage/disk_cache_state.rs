use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::MissedTickBehavior;

pub type ObjectKey = (String, String);

const SHARD_COUNT: usize = 64;
const BATCH_INTERVAL_MS: u64 = 1;
const BATCH_MAX_OPS: usize = 256;

fn shard_index(key: &ObjectKey) -> usize {
    let mut h: u64 = 0;
    for b in key.0.bytes().chain(key.1.bytes()) {
        h = h.wrapping_mul(31).wrapping_add(u64::from(b));
    }
    (h as usize) % SHARD_COUNT
}

struct Entry {
    size: u64,
    dirty: bool,
    prev_clean: Option<ObjectKey>,
    next_clean: Option<ObjectKey>,
}

struct Shard {
    entries: HashMap<ObjectKey, Entry>,
    clean_head: Option<ObjectKey>,
    clean_tail: Option<ObjectKey>,
    total_size: u64,
    dirty_count: usize,
}

impl Shard {
    fn unlink_clean(&mut self, key: &ObjectKey) {
        let Some(entry) = self.entries.get(key) else {
            return;
        };
        if entry.dirty {
            return;
        }
        let (prev, next) = (entry.prev_clean.clone(), entry.next_clean.clone());
        match prev {
            Some(ref p) => {
                if let Some(e) = self.entries.get_mut(p) {
                    e.next_clean = next.clone();
                }
            }
            None => self.clean_head = next.clone(),
        }
        match next {
            Some(ref n) => {
                if let Some(e) = self.entries.get_mut(n) {
                    e.prev_clean = prev;
                }
            }
            None => self.clean_tail = prev,
        }
        if let Some(e) = self.entries.get_mut(key) {
            e.prev_clean = None;
            e.next_clean = None;
        }
    }

    fn link_clean_head(&mut self, key: &ObjectKey) {
        let key = key.clone();
        if self.clean_head.as_ref() == Some(&key) {
            return;
        }
        self.unlink_clean(&key);
        match self.clean_head.take() {
            Some(head) => {
                if let Some(e) = self.entries.get_mut(&head) {
                    e.prev_clean = Some(key.clone());
                }
                if let Some(e) = self.entries.get_mut(&key) {
                    e.prev_clean = None;
                    e.next_clean = Some(head);
                }
                self.clean_head = Some(key);
            }
            None => {
                if let Some(e) = self.entries.get_mut(&key) {
                    e.prev_clean = None;
                    e.next_clean = None;
                }
                self.clean_head = Some(key.clone());
                self.clean_tail = Some(key);
            }
        }
    }

    fn mark_dirty(&mut self, key: ObjectKey, size: u64) -> (i64, i64) {
        let mut dirty_delta = 0i64;
        let entry_delta = if let Some(entry) = self.entries.get_mut(&key) {
            let delta = size as i64 - entry.size as i64;
            self.total_size = self.total_size.saturating_sub(entry.size) + size;
            entry.size = size;
            if !entry.dirty {
                entry.dirty = true;
                self.dirty_count += 1;
                dirty_delta = 1;
                self.unlink_clean(&key);
            }
            delta
        } else {
            self.entries.insert(
                key,
                Entry {
                    size,
                    dirty: true,
                    prev_clean: None,
                    next_clean: None,
                },
            );
            self.total_size += size;
            self.dirty_count += 1;
            dirty_delta = 1;
            size as i64
        };
        (entry_delta, dirty_delta)
    }

    fn mark_clean(&mut self, key: &ObjectKey, size: u64) -> (i64, i64) {
        let mut dirty_delta = 0i64;
        let entry_delta = if let Some(entry) = self.entries.get_mut(key) {
            let delta = size as i64 - entry.size as i64;
            self.total_size = self.total_size.saturating_sub(entry.size) + size;
            entry.size = size;
            if entry.dirty {
                entry.dirty = false;
                self.dirty_count = self.dirty_count.saturating_sub(1);
                dirty_delta = -1;
            }
            self.link_clean_head(key);
            delta
        } else {
            let key = key.clone();
            self.entries.insert(
                key.clone(),
                Entry {
                    size,
                    dirty: false,
                    prev_clean: None,
                    next_clean: None,
                },
            );
            self.total_size += size;
            self.link_clean_head(&key);
            size as i64
        };
        (entry_delta, dirty_delta)
    }

    fn record_hit(&mut self, key: ObjectKey, size: u64) -> i64 {
        if let Some(entry) = self.entries.get_mut(&key) {
            let delta = size as i64 - entry.size as i64;
            self.total_size = self.total_size.saturating_sub(entry.size) + size;
            entry.size = size;
            if !entry.dirty {
                self.link_clean_head(&key);
            }
            return delta;
        }
        self.entries.insert(
            key.clone(),
            Entry {
                size,
                dirty: false,
                prev_clean: None,
                next_clean: None,
            },
        );
        self.total_size += size;
        self.link_clean_head(&key);
        size as i64
    }

    fn remove(&mut self, key: &ObjectKey) -> Option<(u64, bool)> {
        let entry = self.entries.remove(key)?;
        self.total_size = self.total_size.saturating_sub(entry.size);
        if entry.dirty {
            self.dirty_count = self.dirty_count.saturating_sub(1);
        } else {
            self.unlink_clean(key);
        }
        Some((entry.size, entry.dirty))
    }

    fn pop_clean_lru(&mut self) -> Option<(ObjectKey, u64)> {
        let key = self.clean_tail.clone()?;
        let size = self.entries.get(&key)?.size;
        self.unlink_clean(&key);
        self.entries.remove(&key)?;
        self.total_size = self.total_size.saturating_sub(size);
        Some((key, size))
    }

    fn insert_indexed(&mut self, key: ObjectKey, size: u64, dirty: bool) {
        if self.entries.contains_key(&key) {
            return;
        }
        self.entries.insert(
            key.clone(),
            Entry {
                size,
                dirty,
                prev_clean: None,
                next_clean: None,
            },
        );
        self.total_size += size;
        if dirty {
            self.dirty_count += 1;
        } else {
            self.link_clean_head(&key);
        }
    }

    fn retain_bucket(&mut self, bucket: &str) -> (i64, i64) {
        let keys: Vec<ObjectKey> = self
            .entries
            .keys()
            .filter(|(b, _)| b == bucket)
            .cloned()
            .collect();
        let mut entry_delta = 0i64;
        let mut dirty_delta = 0i64;
        for key in keys {
            if let Some((size, dirty)) = self.remove(&key) {
                entry_delta -= size as i64;
                if dirty {
                    dirty_delta -= 1;
                }
            }
        }
        (entry_delta, dirty_delta)
    }
}

pub struct DiskCacheState {
    shards: Vec<Mutex<Shard>>,
    total_size: AtomicU64,
    entry_count: AtomicUsize,
    dirty_count: AtomicUsize,
    dirty_bytes: AtomicU64,
}

impl DiskCacheState {
    pub fn new() -> Self {
        let shards = (0..SHARD_COUNT)
            .map(|_| {
                Mutex::new(Shard {
                    entries: HashMap::new(),
                    clean_head: None,
                    clean_tail: None,
                    total_size: 0,
                    dirty_count: 0,
                })
            })
            .collect();
        Self {
            shards,
            total_size: AtomicU64::new(0),
            entry_count: AtomicUsize::new(0),
            dirty_count: AtomicUsize::new(0),
            dirty_bytes: AtomicU64::new(0),
        }
    }

    fn apply_shard_deltas(&self, entry_delta: i64, dirty_delta: i64, dirty_bytes_delta: i64) {
        if entry_delta != 0 {
            if entry_delta > 0 {
                self.entry_count
                    .fetch_add(entry_delta as usize, Ordering::Relaxed);
            } else {
                self.entry_count
                    .fetch_sub((-entry_delta) as usize, Ordering::Relaxed);
            }
            if entry_delta > 0 {
                self.total_size
                    .fetch_add(entry_delta as u64, Ordering::Relaxed);
            } else {
                self.total_size
                    .fetch_sub((-entry_delta) as u64, Ordering::Relaxed);
            }
        }
        if dirty_delta != 0 {
            if dirty_delta > 0 {
                self.dirty_count
                    .fetch_add(dirty_delta as usize, Ordering::Relaxed);
            } else {
                self.dirty_count
                    .fetch_sub((-dirty_delta) as usize, Ordering::Relaxed);
            }
        }
        if dirty_bytes_delta != 0 {
            if dirty_bytes_delta > 0 {
                self.dirty_bytes
                    .fetch_add(dirty_bytes_delta as u64, Ordering::Relaxed);
            } else {
                self.dirty_bytes
                    .fetch_sub((-dirty_bytes_delta) as u64, Ordering::Relaxed);
            }
        }
    }

    pub fn total_size(&self) -> u64 {
        self.total_size.load(Ordering::Relaxed)
    }

    pub fn entry_count(&self) -> usize {
        self.entry_count.load(Ordering::Relaxed)
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty_count.load(Ordering::Relaxed)
    }

    pub fn dirty_bytes(&self) -> u64 {
        self.dirty_bytes.load(Ordering::Relaxed)
    }

    pub fn mark_dirty_sync(&self, bucket: &str, key: &str, size: u64) {
        let key = (bucket.to_string(), key.to_string());
        let mut shard = self.shards[shard_index(&key)].lock().unwrap();
        let old = shard.entries.get(&key).map(|e| (e.size, e.dirty));
        let (entry_delta, dirty_delta) = shard.mark_dirty(key, size);
        drop(shard);
        let dirty_bytes_delta = match old {
            None => size as i64,
            Some((_old_size, false)) => size as i64,
            Some((old_size, true)) => size as i64 - old_size as i64,
        };
        self.apply_shard_deltas(entry_delta, dirty_delta, dirty_bytes_delta);
    }

    pub fn mark_clean_sync(&self, bucket: &str, key: &str, size: u64) {
        let key = (bucket.to_string(), key.to_string());
        let mut shard = self.shards[shard_index(&key)].lock().unwrap();
        let old_dirty = shard
            .entries
            .get(&key)
            .and_then(|e| e.dirty.then_some(e.size));
        let (entry_delta, dirty_delta) = shard.mark_clean(&key, size);
        drop(shard);
        let dirty_bytes_delta = old_dirty.map(|s| -(s as i64)).unwrap_or(0);
        self.apply_shard_deltas(entry_delta, dirty_delta, dirty_bytes_delta);
    }

    pub fn record_hit_sync(&self, bucket: &str, key: &str, size: u64) {
        let key = (bucket.to_string(), key.to_string());
        let mut shard = self.shards[shard_index(&key)].lock().unwrap();
        let entry_delta = shard.record_hit(key, size);
        drop(shard);
        self.apply_shard_deltas(entry_delta, 0, 0);
    }

    pub fn remove_sync(&self, bucket: &str, key: &str) -> Option<u64> {
        let key = (bucket.to_string(), key.to_string());
        let mut shard = self.shards[shard_index(&key)].lock().unwrap();
        let removed = shard.remove(&key);
        drop(shard);
        if let Some((size, dirty)) = removed {
            self.apply_shard_deltas(-(size as i64), if dirty { -1 } else { 0 }, 0);
            if dirty {
                self.dirty_bytes.fetch_sub(size, Ordering::Relaxed);
            }
            Some(size)
        } else {
            None
        }
    }

    pub fn pop_clean_lru(&self) -> Option<(ObjectKey, u64)> {
        for shard in &self.shards {
            let mut guard = shard.lock().unwrap();
            if let Some(victim) = guard.pop_clean_lru() {
                drop(guard);
                self.entry_count.fetch_sub(1, Ordering::Relaxed);
                self.total_size.fetch_sub(victim.1, Ordering::Relaxed);
                return Some(victim);
            }
        }
        None
    }

    pub fn apply_bulk(&self, entries: &[(String, String, u64)], dirty: &HashSet<ObjectKey>) {
        for shard in &self.shards {
            let mut guard = shard.lock().unwrap();
            guard.entries.clear();
            guard.clean_head = None;
            guard.clean_tail = None;
            guard.total_size = 0;
            guard.dirty_count = 0;
        }
        let mut total_size = 0u64;
        let mut entry_count = 0usize;
        let mut dirty_count = 0usize;
        let mut dirty_bytes = 0u64;
        for (bucket, key, size) in entries {
            let object_key = (bucket.clone(), key.clone());
            let is_dirty = dirty.contains(&object_key);
            let mut shard = self.shards[shard_index(&object_key)].lock().unwrap();
            shard.insert_indexed(object_key, *size, is_dirty);
            total_size += size;
            entry_count += 1;
            if is_dirty {
                dirty_count += 1;
                dirty_bytes += size;
            }
        }
        self.total_size.store(total_size, Ordering::Relaxed);
        self.entry_count.store(entry_count, Ordering::Relaxed);
        self.dirty_count.store(dirty_count, Ordering::Relaxed);
        self.dirty_bytes.store(dirty_bytes, Ordering::Relaxed);
    }

    pub fn all_keys(&self) -> HashSet<ObjectKey> {
        let mut keys = HashSet::new();
        for shard in &self.shards {
            let guard = shard.lock().unwrap();
            keys.extend(guard.entries.keys().cloned());
        }
        keys
    }

    pub fn all_entries(&self) -> Vec<(String, String, u64)> {
        let mut out = Vec::with_capacity(self.entry_count());
        for shard in &self.shards {
            let guard = shard.lock().unwrap();
            for ((bucket, key), entry) in &guard.entries {
                out.push((bucket.clone(), key.clone(), entry.size));
            }
        }
        out
    }

    pub fn all_dirty(&self) -> HashSet<ObjectKey> {
        let mut dirty = HashSet::new();
        for shard in &self.shards {
            let guard = shard.lock().unwrap();
            for (key, entry) in &guard.entries {
                if entry.dirty {
                    dirty.insert(key.clone());
                }
            }
        }
        dirty
    }

    pub fn recalc_dirty_bytes(&self) {
        let mut bytes = 0u64;
        for shard in &self.shards {
            let guard = shard.lock().unwrap();
            for entry in guard.entries.values() {
                if entry.dirty {
                    bytes += entry.size;
                }
            }
        }
        self.dirty_bytes.store(bytes, Ordering::Relaxed);
    }

    pub fn purge_bucket_sync(&self, bucket: &str) {
        let mut entry_delta = 0i64;
        let mut dirty_delta = 0i64;
        for shard in &self.shards {
            let mut guard = shard.lock().unwrap();
            let (e, d) = guard.retain_bucket(bucket);
            entry_delta += e;
            dirty_delta += d;
        }
        self.apply_shard_deltas(entry_delta, dirty_delta, 0);
        self.recalc_dirty_bytes();
    }

    pub fn merge_reconcile(
        &self,
        pre_scan_keys: &HashSet<ObjectKey>,
        on_disk: &HashMap<ObjectKey, u64>,
    ) -> (u64, u64) {
        let mut removed = 0u64;
        for key in pre_scan_keys {
            if on_disk.contains_key(key) {
                continue;
            }
            if self.remove_sync(&key.0, &key.1).is_some() {
                removed += 1;
            }
        }
        let mut added = 0u64;
        for (key, size) in on_disk {
            if pre_scan_keys.contains(key) || self.contains_key(key) {
                continue;
            }
            let mut shard = self.shards[shard_index(key)].lock().unwrap();
            shard.insert_indexed(key.clone(), *size, false);
            drop(shard);
            self.entry_count.fetch_add(1, Ordering::Relaxed);
            self.total_size.fetch_add(*size, Ordering::Relaxed);
            added += 1;
        }
        (removed, added)
    }

    fn contains_key(&self, key: &ObjectKey) -> bool {
        self.shards[shard_index(key)]
            .lock()
            .unwrap()
            .entries
            .contains_key(key)
    }

    pub fn set_dirty_set(&self, dirty: HashSet<ObjectKey>) {
        for shard in &self.shards {
            let mut guard = shard.lock().unwrap();
            guard.clean_head = None;
            guard.clean_tail = None;
            guard.dirty_count = 0;
            let keys: Vec<ObjectKey> = guard.entries.keys().cloned().collect();
            for key in &keys {
                let is_dirty = dirty.contains(key);
                if let Some(entry) = guard.entries.get_mut(key) {
                    entry.dirty = is_dirty;
                    entry.prev_clean = None;
                    entry.next_clean = None;
                    if is_dirty {
                        guard.dirty_count += 1;
                    }
                }
            }
            for key in &keys {
                if !dirty.contains(key) {
                    guard.link_clean_head(key);
                }
            }
        }
        self.dirty_count.store(dirty.len(), Ordering::Relaxed);
        self.recalc_dirty_bytes();
    }
}

enum StateOp {
    MarkDirty {
        bucket: String,
        key: String,
        size: u64,
    },
    RecordHit {
        bucket: String,
        key: String,
        size: u64,
    },
    Remove {
        bucket: String,
        key: String,
    },
    Drain(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct CacheStateHandle {
    op_tx: mpsc::UnboundedSender<StateOp>,
}

impl CacheStateHandle {
    pub fn spawn(state: Arc<DiskCacheState>) -> Self {
        let (op_tx, op_rx) = mpsc::unbounded_channel();
        let handle = Self { op_tx };
        tokio::spawn(run_state_worker(state, op_rx));
        handle
    }

    pub fn mark_dirty(&self, bucket: &str, key: &str, size: u64) {
        let _ = self.op_tx.send(StateOp::MarkDirty {
            bucket: bucket.to_string(),
            key: key.to_string(),
            size,
        });
    }

    pub fn record_hit(&self, bucket: &str, key: &str, size: u64) {
        let _ = self.op_tx.send(StateOp::RecordHit {
            bucket: bucket.to_string(),
            key: key.to_string(),
            size,
        });
    }

    pub async fn remove(&self, bucket: &str, key: &str) {
        let _ = self.op_tx.send(StateOp::Remove {
            bucket: bucket.to_string(),
            key: key.to_string(),
        });
        self.drain().await;
    }

    pub async fn drain(&self) {
        let (tx, rx) = oneshot::channel();
        if self.op_tx.send(StateOp::Drain(tx)).is_err() {
            return;
        }
        let _ = rx.await;
    }
}

async fn run_state_worker(state: Arc<DiskCacheState>, mut rx: mpsc::UnboundedReceiver<StateOp>) {
    let mut pending: HashMap<ObjectKey, StateOp> = HashMap::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(BATCH_INTERVAL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(StateOp::Drain(done)) => {
                        apply_pending(&state, &mut pending);
                        let _ = done.send(());
                    }
                    Some(op) => {
                        let key = match &op {
                            StateOp::MarkDirty { bucket, key, .. } => (bucket.clone(), key.clone()),
                            StateOp::RecordHit { bucket, key, .. } => (bucket.clone(), key.clone()),
                            StateOp::Remove { bucket, key } => (bucket.clone(), key.clone()),
                            StateOp::Drain(_) => unreachable!(),
                        };
                        pending.insert(key, op);
                        if pending.len() >= BATCH_MAX_OPS {
                            apply_pending(&state, &mut pending);
                        }
                    }
                    None => {
                        apply_pending(&state, &mut pending);
                        break;
                    }
                }
            }
            _ = ticker.tick() => {
                if !pending.is_empty() {
                    apply_pending(&state, &mut pending);
                }
            }
        }
    }
}

fn apply_pending(state: &DiskCacheState, pending: &mut HashMap<ObjectKey, StateOp>) {
    for (_, op) in pending.drain() {
        match op {
            StateOp::MarkDirty { bucket, key, size } => state.mark_dirty_sync(&bucket, &key, size),
            StateOp::RecordHit { bucket, key, size } => state.record_hit_sync(&bucket, &key, size),
            StateOp::Remove { bucket, key } => {
                let _ = state.remove_sync(&bucket, &key);
            }
            StateOp::Drain(_) => {}
        }
    }
}
