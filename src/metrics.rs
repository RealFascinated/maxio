use std::time::{Duration, Instant};

use prometheus::{
    CounterVec, Encoder, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};

use crate::stats::BucketStatsCache;

pub struct MetricsRegistry {
    registry: Registry,
    http_requests_total: CounterVec,
    http_duration: HistogramVec,
    storage_duration: HistogramVec,
    cache_hits: prometheus::Counter,
    cache_misses: prometheus::Counter,
    cache_evictions: prometheus::Counter,
    cache_populate_bytes: prometheus::Counter,
    cache_flush_total: CounterVec,
    cache_flush_bytes: prometheus::Counter,
    cache_flush_duration: prometheus::Histogram,
    cache_size_bytes: prometheus::Gauge,
    cache_entries: prometheus::Gauge,
    cache_dirty_objects: prometheus::Gauge,
    cache_max_size_bytes: prometheus::Gauge,
    cache_writeback_halted: prometheus::Gauge,
    cache_enabled: prometheus::Gauge,
    uptime: prometheus::Gauge,
    start_time: Instant,
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

        let uptime = prometheus::Gauge::new("maxio_uptime_seconds", "Server uptime in seconds")?;
        registry.register(Box::new(uptime.clone()))?;

        let cache_hits = prometheus::Counter::new(
            "maxio_cache_hits_total",
            "Object read cache hits",
        )?;
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

        let cache_populate_bytes = prometheus::Counter::new(
            "maxio_cache_populate_bytes_total",
            "Bytes copied into cache on read-through miss",
        )?;
        registry.register(Box::new(cache_populate_bytes.clone()))?;

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
            cache_hits,
            cache_misses,
            cache_evictions,
            cache_populate_bytes,
            cache_flush_total,
            cache_flush_bytes,
            cache_flush_duration,
            cache_size_bytes,
            cache_entries,
            cache_dirty_objects,
            cache_max_size_bytes,
            cache_writeback_halted,
            cache_enabled,
            uptime,
            start_time: Instant::now(),
        })
    }

    pub fn init_cache_metrics(&self, max_size: u64) {
        self.cache_enabled.set(1.0);
        self.cache_max_size_bytes.set(max_size as f64);
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.inc();
    }

    pub fn record_cache_miss(&self, bytes: u64) {
        self.cache_misses.inc();
        self.cache_populate_bytes.inc_by(bytes as f64);
    }

    pub fn record_cache_eviction(&self) {
        self.cache_evictions.inc();
    }

    pub fn set_cache_state(&self, size_bytes: u64, entries: usize, dirty: usize) {
        self.cache_size_bytes.set(size_bytes as f64);
        self.cache_entries.set(entries as f64);
        self.cache_dirty_objects.set(dirty as f64);
    }

    pub fn set_cache_writeback_halted(&self, halted: bool) {
        self.cache_writeback_halted.set(if halted { 1.0 } else { 0.0 });
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
        self.storage_duration
            .with_label_values(&[operation])
            .observe(elapsed.as_secs_f64());
    }

    pub fn update_uptime(&self) {
        self.uptime.set(self.start_time.elapsed().as_secs_f64());
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
