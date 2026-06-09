use std::time::{Duration, Instant};

use prometheus::{
    CounterVec, Encoder, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};

use crate::stats::BucketStatsCache;

use super::cache_name;
use super::snapshot::CacheSnapshot;

pub struct PrometheusMetrics {
    registry: Registry,
    http_requests_total: CounterVec,
    http_duration: HistogramVec,
    storage_duration: HistogramVec,
    metadata_duration: HistogramVec,
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
}

impl PrometheusMetrics {
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
                "LRU evictions when a cache is at capacity",
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
        })
    }

    pub fn uptime_seconds(&self) -> f64 {
        self.uptime.get()
    }

    pub fn set_uptime(&self, seconds: f64) {
        self.uptime.set(seconds);
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn set_process_cpu_usage(&self, ratio: f64) {
        self.process_cpu_usage.set(ratio);
    }

    pub fn process_cpu_usage_ratio(&self) -> f64 {
        self.process_cpu_usage.get()
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

    pub fn record_storage_op(&self, operation: &str, elapsed: Duration) {
        self.storage_duration
            .with_label_values(&[operation])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_metadata_op(&self, operation: &str, elapsed: Duration) {
        self.metadata_duration
            .with_label_values(&[operation])
            .observe(elapsed.as_secs_f64());
    }

    pub fn cache_snapshot(&self, id: &str) -> CacheSnapshot {
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
