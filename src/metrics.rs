use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use prometheus::{
    CounterVec, Encoder, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};
use serde::Serialize;

use crate::stats::BucketStatsCache;

pub mod cache_name {
    pub const OBJECT_DISK: &str = "object_disk";
    pub const BUCKET: &str = "bucket";
    pub const OBJECT_READ: &str = "object_read";
    pub const SIGNING_KEY: &str = "signing_key";
    pub const IAM_ACCESS_KEY: &str = "iam_access_key";
    pub const IAM_USER: &str = "iam_user";
    pub const IAM_POLICIES: &str = "iam_policies";

    pub const ALL: &[&str] = &[
        OBJECT_DISK,
        BUCKET,
        OBJECT_READ,
        SIGNING_KEY,
        IAM_ACCESS_KEY,
        IAM_USER,
        IAM_POLICIES,
    ];

    pub fn display_name(name: &str) -> &'static str {
        match name {
            OBJECT_DISK => "Object disk",
            BUCKET => "Bucket metadata",
            OBJECT_READ => "Object read metadata",
            SIGNING_KEY => "Signing key",
            IAM_ACCESS_KEY => "IAM access key",
            IAM_USER => "IAM user",
            IAM_POLICIES => "IAM policies",
            _ => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageTotalsSnapshot {
    pub bucket_count: usize,
    pub object_count: i64,
    pub size_bytes: i64,
}

impl StorageTotalsSnapshot {
    pub fn from_bucket_stats(stats: &[crate::stats::BucketStat]) -> Self {
        Self {
            bucket_count: stats.len(),
            object_count: stats.iter().map(|s| s.object_count).sum(),
            size_bytes: stats.iter().map(|s| s.size_bytes).sum(),
        }
    }
}

pub const LATENCY_WINDOW_SECS: u64 = 60;

const READ_LATENCY_OPS: &[&str] = &["get_object", "get_object_range"];
const WRITE_LATENCY_OPS: &[&str] = &["put_object", "complete_multipart_upload"];
const READ_THROUGHPUT_OPS: &[&str] = &["get_object", "get_object_range"];
const WRITE_THROUGHPUT_OPS: &[&str] = &["put_object", "upload_part", "complete_multipart_upload"];
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSnapshot {
    pub uptime_seconds: f64,
    pub storage_totals: StorageTotalsSnapshot,
    pub throughput: ThroughputSnapshot,
    pub latency: LatencySnapshot,
    pub ops_totals: OpsTotalsSnapshot,
    pub active_clients: u64,
    pub caches: Vec<CacheSnapshot>,
    pub storage_ops: Vec<StorageOpSnapshot>,
    pub metadata_ops: Vec<MetadataOpSnapshot>,
    pub process: Option<ProcessSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThroughputSnapshot {
    pub window_seconds: u64,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsTotalsSnapshot {
    pub window_seconds: u64,
    pub read_iops: f64,
    pub write_iops: f64,
    pub meta_iops: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencySnapshot {
    pub window_seconds: u64,
    pub read_seconds: Option<f64>,
    pub write_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSnapshot {
    pub id: String,
    pub name: String,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_bytes: u64,
    pub size_bytes: u64,
    pub entries: u64,
    pub dirty_objects: u64,
    pub max_size_bytes: u64,
    pub writeback_halted: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOpSnapshot {
    pub operation: String,
    pub count: u64,
    pub sum_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataOpSnapshot {
    pub operation: String,
    pub count: u64,
    pub sum_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSnapshot {
    pub resident_memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub cpu_usage_percent: f64,
    pub open_fds: u64,
    pub max_fds: u64,
}

struct RawProcessMetrics {
    resident_memory_bytes: u64,
    virtual_memory_bytes: u64,
    cpu_seconds_total: f64,
    open_fds: u64,
    max_fds: u64,
}

struct RollingLatencyWindow {
    window: Duration,
    samples: Mutex<VecDeque<(Instant, f64)>>,
}

impl RollingLatencyWindow {
    fn new(window: Duration) -> Self {
        Self {
            window,
            samples: Mutex::new(VecDeque::new()),
        }
    }

    fn record(&self, elapsed: Duration) {
        let now = Instant::now();
        let mut samples = self.samples.lock().unwrap();
        samples.push_back((now, elapsed.as_secs_f64()));
        Self::prune(&self.window, &mut samples, now);
    }

    fn average_seconds(&self) -> Option<f64> {
        let mut samples = self.samples.lock().unwrap();
        let now = Instant::now();
        Self::prune(&self.window, &mut samples, now);
        if samples.is_empty() {
            return None;
        }
        let sum: f64 = samples.iter().map(|(_, secs)| secs).sum();
        Some(sum / samples.len() as f64)
    }

    fn prune(window: &Duration, samples: &mut VecDeque<(Instant, f64)>, now: Instant) {
        let cutoff = now.checked_sub(*window).unwrap_or(now);
        while samples.front().is_some_and(|(t, _)| *t < cutoff) {
            samples.pop_front();
        }
    }
}

struct RollingBytesWindow {
    window: Duration,
    samples: Mutex<VecDeque<(Instant, u64)>>,
}

impl RollingBytesWindow {
    fn new(window: Duration) -> Self {
        Self {
            window,
            samples: Mutex::new(VecDeque::new()),
        }
    }

    fn record(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let now = Instant::now();
        let mut samples = self.samples.lock().unwrap();
        samples.push_back((now, bytes));
        Self::prune(&self.window, &mut samples, now);
    }

    fn bytes_per_sec(&self) -> f64 {
        let mut samples = self.samples.lock().unwrap();
        let now = Instant::now();
        Self::prune(&self.window, &mut samples, now);
        let total_bytes: u64 = samples.iter().map(|(_, bytes)| bytes).sum();
        let window_secs = self.window.as_secs_f64();
        if window_secs <= 0.0 {
            return 0.0;
        }
        total_bytes as f64 / window_secs
    }

    fn prune(window: &Duration, samples: &mut VecDeque<(Instant, u64)>, now: Instant) {
        let cutoff = now.checked_sub(*window).unwrap_or(now);
        while samples.front().is_some_and(|(t, _)| *t < cutoff) {
            samples.pop_front();
        }
    }
}

struct RollingCountWindow {
    window: Duration,
    events: Mutex<VecDeque<Instant>>,
}

impl RollingCountWindow {
    fn new(window: Duration) -> Self {
        Self {
            window,
            events: Mutex::new(VecDeque::new()),
        }
    }

    fn record(&self) {
        let now = Instant::now();
        let mut events = self.events.lock().unwrap();
        events.push_back(now);
        Self::prune(&self.window, &mut events, now);
    }

    fn ops_per_sec(&self) -> f64 {
        let mut events = self.events.lock().unwrap();
        let now = Instant::now();
        Self::prune(&self.window, &mut events, now);
        let window_secs = self.window.as_secs_f64();
        if window_secs <= 0.0 {
            return 0.0;
        }
        events.len() as f64 / window_secs
    }

    fn prune(window: &Duration, events: &mut VecDeque<Instant>, now: Instant) {
        let cutoff = now.checked_sub(*window).unwrap_or(now);
        while events.front().is_some_and(|t| *t < cutoff) {
            events.pop_front();
        }
    }
}

pub struct MetricsRegistry {
    registry: Registry,
    http_requests_total: CounterVec,
    http_duration: HistogramVec,
    storage_duration: HistogramVec,
    metadata_duration: HistogramVec,
    storage_op_stats: Mutex<HashMap<String, (u64, f64)>>,
    metadata_op_stats: Mutex<HashMap<String, (u64, f64)>>,
    cache_hits: CounterVec,
    cache_misses: CounterVec,
    cache_evictions: CounterVec,
    cache_flush_total: CounterVec,
    cache_flush_bytes: CounterVec,
    cache_flush_duration: HistogramVec,
    cache_size_bytes: GaugeVec,
    cache_entries: GaugeVec,
    cache_dirty_objects: GaugeVec,
    cache_dirty_bytes: GaugeVec,
    cache_max_size_bytes: GaugeVec,
    cache_writeback_halted: GaugeVec,
    cache_enabled: GaugeVec,
    uptime: prometheus::Gauge,
    process_cpu_usage: prometheus::Gauge,
    start_time: Instant,
    last_process_cpu: Mutex<Option<(f64, Instant)>>,
    read_latency: RollingLatencyWindow,
    write_latency: RollingLatencyWindow,
    read_throughput: RollingBytesWindow,
    write_throughput: RollingBytesWindow,
    drive_read_ops: RollingCountWindow,
    drive_write_ops: RollingCountWindow,
    meta_ops: RollingCountWindow,
    active_s3_requests: AtomicUsize,
}

impl MetricsRegistry {
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        #[cfg(target_os = "linux")]
        {
            let pc = prometheus::process_collector::ProcessCollector::for_self();
            let _ = registry.register(Box::new(pc));
        }

        let http_requests_total = CounterVec::new(
            Opts::new("maxio_http_requests_total", "Total HTTP requests"),
            &["method", "route", "status"],
        )?;
        registry.register(Box::new(http_requests_total.clone()))?;

        let http_duration = HistogramVec::new(
            HistogramOpts::new(
                "maxio_http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.025, 0.1, 0.5, 1.0, 5.0, 10.0]),
            &["method", "route"],
        )?;
        registry.register(Box::new(http_duration.clone()))?;

        let storage_duration = HistogramVec::new(
            HistogramOpts::new(
                "maxio_storage_operation_duration_seconds",
                "Storage operation duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.025, 0.1, 0.5, 1.0, 5.0, 30.0]),
            &["operation"],
        )?;
        registry.register(Box::new(storage_duration.clone()))?;

        let metadata_duration = HistogramVec::new(
            HistogramOpts::new(
                "maxio_metadata_operation_duration_seconds",
                "Postgres metadata operation duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.025, 0.1, 0.5, 1.0, 5.0, 30.0]),
            &["operation"],
        )?;
        registry.register(Box::new(metadata_duration.clone()))?;

        let uptime = prometheus::Gauge::new("maxio_uptime_seconds", "Server uptime in seconds")?;
        registry.register(Box::new(uptime.clone()))?;

        let process_cpu_usage = prometheus::Gauge::new(
            "maxio_process_cpu_usage_ratio",
            "Process CPU usage as a fraction of total machine capacity (0-1)",
        )?;
        registry.register(Box::new(process_cpu_usage.clone()))?;

        let cache_hits = CounterVec::new(
            Opts::new("maxio_cache_hits_total", "Cache hits"),
            &["cache"],
        )?;
        registry.register(Box::new(cache_hits.clone()))?;

        let cache_misses = CounterVec::new(
            Opts::new("maxio_cache_misses_total", "Cache misses"),
            &["cache"],
        )?;
        registry.register(Box::new(cache_misses.clone()))?;

        let cache_evictions = CounterVec::new(
            Opts::new(
                "maxio_cache_evictions_total",
                "Cache evictions or invalidations",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_evictions.clone()))?;

        let cache_flush_total = CounterVec::new(
            Opts::new("maxio_cache_flush_total", "Writeback flush runs"),
            &["cache", "result"],
        )?;
        registry.register(Box::new(cache_flush_total.clone()))?;

        let cache_flush_bytes = CounterVec::new(
            Opts::new(
                "maxio_cache_flush_bytes_total",
                "Bytes flushed from cache to data directory",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_flush_bytes.clone()))?;

        let cache_flush_duration = HistogramVec::new(
            HistogramOpts::new(
                "maxio_cache_flush_duration_seconds",
                "Writeback flush duration in seconds",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0]),
            &["cache"],
        )?;
        registry.register(Box::new(cache_flush_duration.clone()))?;

        let cache_size_bytes = GaugeVec::new(
            Opts::new(
                "maxio_cache_size_bytes",
                "Current tracked cache size in bytes",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_size_bytes.clone()))?;

        let cache_entries = GaugeVec::new(
            Opts::new(
                "maxio_cache_entries",
                "Number of entries tracked in the cache",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_entries.clone()))?;

        let cache_dirty_objects = GaugeVec::new(
            Opts::new(
                "maxio_cache_dirty_objects",
                "Dirty objects awaiting writeback flush",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_dirty_objects.clone()))?;

        let cache_dirty_bytes = GaugeVec::new(
            Opts::new(
                "maxio_cache_dirty_bytes",
                "Bytes in dirty objects awaiting writeback flush",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_dirty_bytes.clone()))?;

        let cache_max_size_bytes = GaugeVec::new(
            Opts::new(
                "maxio_cache_max_size_bytes",
                "Configured maximum cache size in bytes",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_max_size_bytes.clone()))?;

        let cache_writeback_halted = GaugeVec::new(
            Opts::new(
                "maxio_cache_writeback_halted",
                "1 when writeback is halted due to flush failures",
            ),
            &["cache"],
        )?;
        registry.register(Box::new(cache_writeback_halted.clone()))?;

        let cache_enabled = GaugeVec::new(
            Opts::new("maxio_cache_enabled", "1 when the cache is active"),
            &["cache"],
        )?;
        registry.register(Box::new(cache_enabled.clone()))?;

        Ok(Self {
            registry,
            http_requests_total,
            http_duration,
            storage_duration,
            metadata_duration,
            storage_op_stats: Mutex::new(HashMap::new()),
            metadata_op_stats: Mutex::new(HashMap::new()),
            cache_hits,
            cache_misses,
            cache_evictions,
            cache_flush_total,
            cache_flush_bytes,
            cache_flush_duration,
            cache_size_bytes,
            cache_entries,
            cache_dirty_objects,
            cache_dirty_bytes,
            cache_max_size_bytes,
            cache_writeback_halted,
            cache_enabled,
            uptime,
            process_cpu_usage,
            start_time: Instant::now(),
            last_process_cpu: Mutex::new(None),
            read_latency: RollingLatencyWindow::new(Duration::from_secs(LATENCY_WINDOW_SECS)),
            write_latency: RollingLatencyWindow::new(Duration::from_secs(LATENCY_WINDOW_SECS)),
            read_throughput: RollingBytesWindow::new(Duration::from_secs(LATENCY_WINDOW_SECS)),
            write_throughput: RollingBytesWindow::new(Duration::from_secs(LATENCY_WINDOW_SECS)),
            drive_read_ops: RollingCountWindow::new(Duration::from_secs(LATENCY_WINDOW_SECS)),
            drive_write_ops: RollingCountWindow::new(Duration::from_secs(LATENCY_WINDOW_SECS)),
            meta_ops: RollingCountWindow::new(Duration::from_secs(LATENCY_WINDOW_SECS)),
            active_s3_requests: AtomicUsize::new(0),
        })
    }

    pub fn begin_s3_request(&self) {
        self.active_s3_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn end_s3_request(&self) {
        self.active_s3_requests.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn init_object_disk_cache(&self, max_size: u64) {
        let cache = cache_name::OBJECT_DISK;
        self.cache_enabled.with_label_values(&[cache]).set(1.0);
        self.cache_max_size_bytes
            .with_label_values(&[cache])
            .set(max_size as f64);
    }

    pub fn record_cache_hit(&self, cache: &str) {
        self.cache_hits.with_label_values(&[cache]).inc();
    }

    pub fn record_cache_miss(&self, cache: &str) {
        self.cache_misses.with_label_values(&[cache]).inc();
    }

    pub fn record_cache_eviction(&self, cache: &str) {
        self.cache_evictions.with_label_values(&[cache]).inc();
    }

    pub fn record_cache_evictions(&self, cache: &str, count: u64) {
        if count > 0 {
            self.cache_evictions
                .with_label_values(&[cache])
                .inc_by(count as f64);
        }
    }

    pub fn set_cache_entries(&self, cache: &str, entries: usize) {
        self.cache_entries
            .with_label_values(&[cache])
            .set(entries as f64);
    }

    pub fn set_cache_state(
        &self,
        cache: &str,
        size_bytes: u64,
        entries: usize,
        dirty_objects: usize,
        dirty_bytes: u64,
    ) {
        self.cache_size_bytes
            .with_label_values(&[cache])
            .set(size_bytes as f64);
        self.set_cache_entries(cache, entries);
        self.cache_dirty_objects
            .with_label_values(&[cache])
            .set(dirty_objects as f64);
        self.cache_dirty_bytes
            .with_label_values(&[cache])
            .set(dirty_bytes as f64);
    }

    pub fn set_cache_writeback_halted(&self, cache: &str, halted: bool) {
        self.cache_writeback_halted
            .with_label_values(&[cache])
            .set(if halted { 1.0 } else { 0.0 });
    }

    pub fn record_cache_flush(&self, cache: &str, success: bool, bytes: u64, elapsed: Duration) {
        let result = if success { "success" } else { "failure" };
        self.cache_flush_total
            .with_label_values(&[cache, result])
            .inc();
        if success {
            self.cache_flush_bytes
                .with_label_values(&[cache])
                .inc_by(bytes as f64);
        }
        self.cache_flush_duration
            .with_label_values(&[cache])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_http(&self, method: &str, route: &str, status: &str, elapsed: Duration) {
        self.http_requests_total
            .with_label_values(&[method, route, status])
            .inc();
        self.http_duration
            .with_label_values(&[method, route])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_storage_op(&self, operation: &str, elapsed: Duration, bytes: u64) {
        let secs = elapsed.as_secs_f64();
        self.storage_duration
            .with_label_values(&[operation])
            .observe(secs);
        let mut stats = self.storage_op_stats.lock().unwrap();
        let entry = stats.entry(operation.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += secs;
        if READ_LATENCY_OPS.contains(&operation) {
            self.read_latency.record(elapsed);
        } else if WRITE_LATENCY_OPS.contains(&operation) {
            self.write_latency.record(elapsed);
        }
        if READ_THROUGHPUT_OPS.contains(&operation) {
            self.read_throughput.record(bytes);
        } else if WRITE_THROUGHPUT_OPS.contains(&operation) {
            self.write_throughput.record(bytes);
        }
    }

    pub fn record_drive_read_op(&self) {
        self.drive_read_ops.record();
    }

    pub fn record_drive_write_op(&self) {
        self.drive_write_ops.record();
    }

    pub fn record_metadata_op(&self, operation: &str, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        self.metadata_duration
            .with_label_values(&[operation])
            .observe(secs);
        let mut stats = self.metadata_op_stats.lock().unwrap();
        let entry = stats.entry(operation.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += secs;
        self.meta_ops.record();
    }

    pub fn update_uptime(&self) {
        self.uptime.set(self.start_time.elapsed().as_secs_f64());
        if let Some(raw) = read_raw_process_metrics() {
            let ratio =
                self.compute_process_cpu_usage_ratio(raw.cpu_seconds_total, self.uptime.get());
            self.process_cpu_usage.set(ratio);
        }
    }

    fn cache_snapshot(&self, id: &str) -> CacheSnapshot {
        let disk = id == cache_name::OBJECT_DISK;
        CacheSnapshot {
            id: id.to_string(),
            name: cache_name::display_name(id).to_string(),
            hits: self.cache_hits.with_label_values(&[id]).get() as u64,
            misses: self.cache_misses.with_label_values(&[id]).get() as u64,
            evictions: self.cache_evictions.with_label_values(&[id]).get() as u64,
            dirty_bytes: self.cache_dirty_bytes.with_label_values(&[id]).get() as u64,
            size_bytes: self.cache_size_bytes.with_label_values(&[id]).get() as u64,
            entries: self.cache_entries.with_label_values(&[id]).get() as u64,
            dirty_objects: self.cache_dirty_objects.with_label_values(&[id]).get() as u64,
            max_size_bytes: self.cache_max_size_bytes.with_label_values(&[id]).get() as u64,
            writeback_halted: self.cache_writeback_halted.with_label_values(&[id]).get() > 0.0,
            enabled: if disk {
                self.cache_enabled.with_label_values(&[id]).get() > 0.0
            } else {
                true
            },
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        self.update_uptime();

        let caches = cache_name::ALL
            .iter()
            .map(|id| self.cache_snapshot(id))
            .collect();

        let storage_ops = {
            let stats = self.storage_op_stats.lock().unwrap();
            let mut ops: Vec<_> = stats
                .iter()
                .map(|(operation, (count, sum))| StorageOpSnapshot {
                    operation: operation.clone(),
                    count: *count,
                    sum_seconds: *sum,
                })
                .collect();
            ops.sort_by(|a, b| a.operation.cmp(&b.operation));
            ops
        };

        let metadata_ops = {
            let stats = self.metadata_op_stats.lock().unwrap();
            let mut ops: Vec<_> = stats
                .iter()
                .map(|(operation, (count, sum))| MetadataOpSnapshot {
                    operation: operation.clone(),
                    count: *count,
                    sum_seconds: *sum,
                })
                .collect();
            ops.sort_by(|a, b| a.operation.cmp(&b.operation));
            ops
        };

        let uptime_seconds = self.uptime.get();
        let cpu_usage_percent = self.process_cpu_usage.get() * 100.0;
        let process = read_raw_process_metrics().map(|raw| ProcessSnapshot {
            resident_memory_bytes: raw.resident_memory_bytes,
            virtual_memory_bytes: raw.virtual_memory_bytes,
            cpu_usage_percent,
            open_fds: raw.open_fds,
            max_fds: raw.max_fds,
        });

        MetricsSnapshot {
            uptime_seconds,
            storage_totals: StorageTotalsSnapshot {
                bucket_count: 0,
                object_count: 0,
                size_bytes: 0,
            },
            throughput: ThroughputSnapshot {
                window_seconds: LATENCY_WINDOW_SECS,
                read_bytes_per_sec: self.read_throughput.bytes_per_sec(),
                write_bytes_per_sec: self.write_throughput.bytes_per_sec(),
            },
            latency: LatencySnapshot {
                window_seconds: LATENCY_WINDOW_SECS,
                read_seconds: self.read_latency.average_seconds(),
                write_seconds: self.write_latency.average_seconds(),
            },
            ops_totals: OpsTotalsSnapshot {
                window_seconds: LATENCY_WINDOW_SECS,
                read_iops: self.drive_read_ops.ops_per_sec(),
                write_iops: self.drive_write_ops.ops_per_sec(),
                meta_iops: self.meta_ops.ops_per_sec(),
            },
            active_clients: self.active_s3_requests.load(Ordering::Relaxed) as u64,
            caches,
            storage_ops,
            metadata_ops,
            process,
        }
    }

    fn compute_process_cpu_usage_ratio(&self, cpu_seconds_total: f64, uptime_seconds: f64) -> f64 {
        let now = Instant::now();
        let mut last = self.last_process_cpu.lock().unwrap();

        let raw_ratio = match *last {
            Some((prev_cpu, prev_at)) => {
                let elapsed = prev_at.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    (cpu_seconds_total - prev_cpu) / elapsed
                } else if uptime_seconds > 0.0 {
                    cpu_seconds_total / uptime_seconds
                } else {
                    0.0
                }
            }
            None if uptime_seconds > 0.0 => cpu_seconds_total / uptime_seconds,
            None => 0.0,
        };

        *last = Some((cpu_seconds_total, now));
        (raw_ratio / cpu_cores()).max(0.0)
    }

    /// Encode all metrics to Prometheus text format. Bucket stats are read
    /// from the cache and written as transient gauges at scrape time so that
    /// deleted buckets never leave stale label sets in the output.
    pub fn gather_text(&self, stats: &BucketStatsCache) -> String {
        self.update_uptime();
        let encoder = TextEncoder::new();
        let mut all_families = self.registry.gather();

        let bucket_data = stats.get_all();
        if !bucket_data.is_empty() {
            let tmp = Registry::new();
            let obj_gauge = GaugeVec::new(
                Opts::new("maxio_bucket_objects_total", "Number of objects per bucket"),
                &["bucket"],
            )
            .unwrap();
            let size_gauge = GaugeVec::new(
                Opts::new("maxio_bucket_size_bytes", "Total size in bytes per bucket"),
                &["bucket"],
            )
            .unwrap();
            let _ = tmp.register(Box::new(obj_gauge.clone()));
            let _ = tmp.register(Box::new(size_gauge.clone()));

            for stat in &bucket_data {
                obj_gauge
                    .with_label_values(&[&stat.name])
                    .set(stat.object_count as f64);
                size_gauge
                    .with_label_values(&[&stat.name])
                    .set(stat.size_bytes as f64);
            }
            all_families.extend(tmp.gather());
        }

        let mut buf = Vec::new();
        let _ = encoder.encode(&all_families, &mut buf);
        String::from_utf8(buf).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_latency_averages_recent_samples() {
        let window = RollingLatencyWindow::new(Duration::from_secs(15));
        window.record(Duration::from_millis(100));
        window.record(Duration::from_millis(200));
        let avg = window.average_seconds().unwrap();
        assert!((avg - 0.15).abs() < 0.001);
    }

    #[test]
    fn rolling_bytes_window_computes_rate() {
        let window = RollingBytesWindow::new(Duration::from_secs(10));
        window.record(1_000);
        window.record(2_000);
        let rate = window.bytes_per_sec();
        assert!((rate - 300.0).abs() < 0.001);
    }

    #[test]
    fn rolling_count_window_computes_ops_per_sec() {
        let window = RollingCountWindow::new(Duration::from_secs(10));
        window.record();
        window.record();
        window.record();
        let rate = window.ops_per_sec();
        assert!((rate - 0.3).abs() < 0.001);
    }
}

fn cpu_cores() -> f64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0)
}

fn read_raw_process_metrics() -> Option<RawProcessMetrics> {
    #[cfg(target_os = "linux")]
    {
        read_linux_process_metrics()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn read_linux_process_metrics() -> Option<RawProcessMetrics> {
    const CLOCK_TICKS_PER_SEC: f64 = 100.0;

    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let resident_kb = parse_proc_status_kb(&status, "VmRSS:")?;
    let virtual_kb = parse_proc_status_kb(&status, "VmSize:")?;

    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let (utime, stime) = parse_proc_stat_cpu(&stat)?;
    let cpu_seconds_total = (utime + stime) as f64 / CLOCK_TICKS_PER_SEC;

    let open_fds = std::fs::read_dir("/proc/self/fd").ok()?.count() as u64;
    let max_fds = parse_proc_max_open_files().unwrap_or(0);

    Some(RawProcessMetrics {
        resident_memory_bytes: resident_kb * 1024,
        virtual_memory_bytes: virtual_kb * 1024,
        cpu_seconds_total,
        open_fds,
        max_fds,
    })
}

#[cfg(target_os = "linux")]
fn parse_proc_status_kb(status: &str, key: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let (label, value) = line.split_once(':')?;
        if label.trim() != key.trim_end_matches(':') {
            return None;
        }
        let kb = value.split_whitespace().next()?.parse().ok()?;
        Some(kb)
    })
}

#[cfg(target_os = "linux")]
fn parse_proc_stat_cpu(stat: &str) -> Option<(u64, u64)> {
    let close_paren = stat.rfind(')')?;
    let rest = stat[close_paren + 1..].trim_start();
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let utime = fields.get(11)?.parse().ok()?;
    let stime = fields.get(12)?.parse().ok()?;
    Some((utime, stime))
}

#[cfg(target_os = "linux")]
fn parse_proc_max_open_files() -> Option<u64> {
    let limits = std::fs::read_to_string("/proc/self/limits").ok()?;
    for line in limits.lines() {
        if line.starts_with("Max open files") {
            let soft = line.split_whitespace().nth(3)?;
            return soft.parse().ok();
        }
    }
    None
}
