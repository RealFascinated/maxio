use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use prometheus::{
    CounterVec, Encoder, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};
use serde::Serialize;

use crate::stats::BucketStatsCache;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSnapshot {
    pub uptime_seconds: f64,
    pub cache: CacheSnapshot,
    pub storage_ops: Vec<StorageOpSnapshot>,
    pub metadata_ops: Vec<MetadataOpSnapshot>,
    pub process: Option<ProcessSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSnapshot {
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

pub struct MetricsRegistry {
    registry: Registry,
    http_requests_total: CounterVec,
    http_duration: HistogramVec,
    storage_duration: HistogramVec,
    metadata_duration: HistogramVec,
    storage_op_stats: Mutex<HashMap<String, (u64, f64)>>,
    metadata_op_stats: Mutex<HashMap<String, (u64, f64)>>,
    cache_hits: prometheus::Counter,
    cache_misses: prometheus::Counter,
    cache_evictions: prometheus::Counter,
    cache_flush_total: CounterVec,
    cache_flush_bytes: prometheus::Counter,
    cache_flush_duration: prometheus::Histogram,
    cache_size_bytes: prometheus::Gauge,
    cache_entries: prometheus::Gauge,
    cache_dirty_objects: prometheus::Gauge,
    cache_dirty_bytes: prometheus::Gauge,
    cache_max_size_bytes: prometheus::Gauge,
    cache_writeback_halted: prometheus::Gauge,
    cache_enabled: prometheus::Gauge,
    uptime: prometheus::Gauge,
    start_time: Instant,
    last_process_cpu: Mutex<Option<(f64, Instant)>>,
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

        let cache_hits =
            prometheus::Counter::new("maxio_cache_hits_total", "Object read cache hits")?;
        registry.register(Box::new(cache_hits.clone()))?;

        let cache_misses = prometheus::Counter::new(
            "maxio_cache_misses_total",
            "Object read cache misses (read-through populate)",
        )?;
        registry.register(Box::new(cache_misses.clone()))?;

        let cache_evictions = prometheus::Counter::new(
            "maxio_cache_evictions_total",
            "LRU cache evictions of clean objects",
        )?;
        registry.register(Box::new(cache_evictions.clone()))?;

        let cache_flush_total = CounterVec::new(
            Opts::new("maxio_cache_flush_total", "Writeback flush runs"),
            &["result"],
        )?;
        registry.register(Box::new(cache_flush_total.clone()))?;

        let cache_flush_bytes = prometheus::Counter::new(
            "maxio_cache_flush_bytes_total",
            "Bytes flushed from cache to data directory",
        )?;
        registry.register(Box::new(cache_flush_bytes.clone()))?;

        let cache_flush_duration = prometheus::Histogram::with_opts(
            HistogramOpts::new(
                "maxio_cache_flush_duration_seconds",
                "Writeback flush duration in seconds",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0]),
        )?;
        registry.register(Box::new(cache_flush_duration.clone()))?;

        let cache_size_bytes = prometheus::Gauge::new(
            "maxio_cache_size_bytes",
            "Current tracked cache size in bytes",
        )?;
        registry.register(Box::new(cache_size_bytes.clone()))?;

        let cache_entries = prometheus::Gauge::new(
            "maxio_cache_entries",
            "Number of objects tracked in the cache LRU",
        )?;
        registry.register(Box::new(cache_entries.clone()))?;

        let cache_dirty_objects = prometheus::Gauge::new(
            "maxio_cache_dirty_objects",
            "Dirty objects awaiting writeback flush",
        )?;
        registry.register(Box::new(cache_dirty_objects.clone()))?;

        let cache_dirty_bytes = prometheus::Gauge::new(
            "maxio_cache_dirty_bytes",
            "Bytes in dirty objects awaiting writeback flush",
        )?;
        registry.register(Box::new(cache_dirty_bytes.clone()))?;

        let cache_max_size_bytes = prometheus::Gauge::new(
            "maxio_cache_max_size_bytes",
            "Configured maximum cache size in bytes",
        )?;
        registry.register(Box::new(cache_max_size_bytes.clone()))?;

        let cache_writeback_halted = prometheus::Gauge::new(
            "maxio_cache_writeback_halted",
            "1 when writeback is halted due to flush failures",
        )?;
        registry.register(Box::new(cache_writeback_halted.clone()))?;

        let cache_enabled = prometheus::Gauge::new(
            "maxio_cache_enabled",
            "1 when an object cache directory is configured",
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
            start_time: Instant::now(),
            last_process_cpu: Mutex::new(None),
        })
    }

    pub fn init_cache_metrics(&self, max_size: u64) {
        self.cache_enabled.set(1.0);
        self.cache_max_size_bytes.set(max_size as f64);
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.inc();
    }

    pub fn record_cache_miss(&self) {
        self.cache_misses.inc();
    }

    pub fn record_cache_eviction(&self) {
        self.cache_evictions.inc();
    }

    pub fn set_cache_state(
        &self,
        size_bytes: u64,
        entries: usize,
        dirty_objects: usize,
        dirty_bytes: u64,
    ) {
        self.cache_size_bytes.set(size_bytes as f64);
        self.cache_entries.set(entries as f64);
        self.cache_dirty_objects.set(dirty_objects as f64);
        self.cache_dirty_bytes.set(dirty_bytes as f64);
    }

    pub fn set_cache_writeback_halted(&self, halted: bool) {
        self.cache_writeback_halted
            .set(if halted { 1.0 } else { 0.0 });
    }

    pub fn record_cache_flush(&self, success: bool, bytes: u64, elapsed: Duration) {
        let result = if success { "success" } else { "failure" };
        self.cache_flush_total.with_label_values(&[result]).inc();
        if success {
            self.cache_flush_bytes.inc_by(bytes as f64);
        }
        self.cache_flush_duration.observe(elapsed.as_secs_f64());
    }

    pub fn record_http(&self, method: &str, route: &str, status: &str, elapsed: Duration) {
        self.http_requests_total
            .with_label_values(&[method, route, status])
            .inc();
        self.http_duration
            .with_label_values(&[method, route])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_storage_op(&self, operation: &str, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        self.storage_duration
            .with_label_values(&[operation])
            .observe(secs);
        let mut stats = self.storage_op_stats.lock().unwrap();
        let entry = stats.entry(operation.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += secs;
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
    }

    pub fn update_uptime(&self) {
        self.uptime.set(self.start_time.elapsed().as_secs_f64());
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        self.update_uptime();

        let cache = CacheSnapshot {
            hits: self.cache_hits.get() as u64,
            misses: self.cache_misses.get() as u64,
            evictions: self.cache_evictions.get() as u64,
            dirty_bytes: self.cache_dirty_bytes.get() as u64,
            size_bytes: self.cache_size_bytes.get() as u64,
            entries: self.cache_entries.get() as u64,
            dirty_objects: self.cache_dirty_objects.get() as u64,
            max_size_bytes: self.cache_max_size_bytes.get() as u64,
            writeback_halted: self.cache_writeback_halted.get() > 0.0,
            enabled: self.cache_enabled.get() > 0.0,
        };

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
        let process = read_raw_process_metrics().map(|raw| ProcessSnapshot {
            resident_memory_bytes: raw.resident_memory_bytes,
            virtual_memory_bytes: raw.virtual_memory_bytes,
            cpu_usage_percent: self
                .process_cpu_usage_percent(raw.cpu_seconds_total, uptime_seconds),
            open_fds: raw.open_fds,
            max_fds: raw.max_fds,
        });

        MetricsSnapshot {
            uptime_seconds,
            cache,
            storage_ops,
            metadata_ops,
            process,
        }
    }

    fn process_cpu_usage_percent(&self, cpu_seconds_total: f64, uptime_seconds: f64) -> f64 {
        let now = Instant::now();
        let mut last = self.last_process_cpu.lock().unwrap();

        let percent = match *last {
            Some((prev_cpu, prev_at)) => {
                let elapsed = prev_at.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    ((cpu_seconds_total - prev_cpu) / elapsed) * 100.0
                } else if uptime_seconds > 0.0 {
                    (cpu_seconds_total / uptime_seconds) * 100.0
                } else {
                    0.0
                }
            }
            None if uptime_seconds > 0.0 => (cpu_seconds_total / uptime_seconds) * 100.0,
            None => 0.0,
        };

        *last = Some((cpu_seconds_total, now));
        percent.max(0.0)
    }

    /// Encode all metrics to Prometheus text format. Bucket stats are read
    /// from the cache and written as transient gauges at scrape time so that
    /// deleted buckets never leave stale label sets in the output.
    pub fn gather_text(&self, stats: &BucketStatsCache) -> String {
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
