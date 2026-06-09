pub mod cache_name;
mod console;
mod process;
mod prometheus;
mod rolling;
mod snapshot;

#[allow(unused_imports)]
pub use snapshot::{
    CacheSnapshot, LATENCY_WINDOW_SECS, LatencySnapshot, MetadataOpSnapshot, MetricsSnapshot,
    OpsTotalsSnapshot, ProcessSnapshot, StorageOpSnapshot, StorageTotalsSnapshot,
    ThroughputSnapshot,
};

use std::time::Duration;

use crate::stats::BucketStatsCache;

use console::ConsoleMetrics;
use prometheus::PrometheusMetrics;

pub struct MetricsRegistry {
    prom: PrometheusMetrics,
    console: ConsoleMetrics,
}

impl MetricsRegistry {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            prom: PrometheusMetrics::new()?,
            console: ConsoleMetrics::new(),
        })
    }

    pub fn begin_s3_request(&self) {
        self.console.begin_s3_request();
    }

    pub fn end_s3_request(&self) {
        self.console.end_s3_request();
    }

    pub fn init_object_disk_cache(&self, max_size: u64) {
        self.prom.init_object_disk_cache(max_size);
    }

    pub fn record_cache_hit(&self, cache: &str) {
        self.prom.record_cache_hit(cache);
    }

    pub fn record_cache_miss(&self, cache: &str) {
        self.prom.record_cache_miss(cache);
    }

    pub fn record_cache_eviction(&self, cache: &str) {
        self.prom.record_cache_eviction(cache);
    }

    pub fn set_cache_entries(&self, cache: &str, entries: usize) {
        self.prom.set_cache_entries(cache, entries);
    }

    pub fn set_cache_state(
        &self,
        cache: &str,
        size_bytes: u64,
        entries: usize,
        dirty_objects: usize,
        dirty_bytes: u64,
    ) {
        self.prom
            .set_cache_state(cache, size_bytes, entries, dirty_objects, dirty_bytes);
    }

    pub fn set_cache_writeback_halted(&self, cache: &str, halted: bool) {
        self.prom.set_cache_writeback_halted(cache, halted);
    }

    pub fn record_cache_flush(&self, cache: &str, success: bool, bytes: u64, elapsed: Duration) {
        self.prom.record_cache_flush(cache, success, bytes, elapsed);
    }

    pub fn record_http(&self, method: &str, route: &str, status: &str, elapsed: Duration) {
        self.prom.record_http(method, route, status, elapsed);
    }

    pub fn record_storage_op(&self, operation: &str, elapsed: Duration, bytes: u64) {
        self.prom.record_storage_op(operation, elapsed);
        self.console.record_storage_op(operation, elapsed, bytes);
    }

    pub fn record_drive_read_op(&self) {
        self.console.record_drive_read_op();
    }

    pub fn record_drive_write_op(&self) {
        self.console.record_drive_write_op();
    }

    pub fn record_metadata_op(&self, operation: &str, elapsed: Duration) {
        self.prom.record_metadata_op(operation, elapsed);
        self.console.record_metadata_op(operation, elapsed);
    }

    pub fn update_uptime(&self) {
        self.prom.set_uptime(self.prom.elapsed().as_secs_f64());
        self.console.update_process_cpu(&self.prom);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        self.update_uptime();
        self.console.snapshot(&self.prom)
    }

    pub fn gather_text(&self, stats: &BucketStatsCache) -> String {
        self.update_uptime();
        self.prom.gather_text(stats)
    }
}
