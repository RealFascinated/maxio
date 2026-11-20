use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use super::cache_name;
use super::histogram::{HistogramCounts, LatencyHistogram};
use super::process::{ProcessCpuTracker, read_raw_process_metrics};
use super::prometheus::PrometheusMetrics;
use super::rolling::{RollingBytesWindow, RollingCountWindow, RollingLatencyWindow};
use super::snapshot::{
    HttpSnapshot, LATENCY_WINDOW_SECS, LatencyPercentilesSnapshot, LatencySnapshot,
    MetadataOpSnapshot, MetricsSnapshot, OpsTotalsSnapshot, StorageOpSnapshot,
    StorageTotalsSnapshot, ThroughputSnapshot,
};

const READ_LATENCY_OPS: &[&str] = &["get_object", "get_object_range"];
const WRITE_LATENCY_OPS: &[&str] = &["put_object", "complete_multipart_upload"];
const READ_THROUGHPUT_OPS: &[&str] = &["get_object", "get_object_range"];
const WRITE_THROUGHPUT_OPS: &[&str] = &["put_object", "upload_part", "complete_multipart_upload"];

/// Cumulative count, latency sum and latency histogram for one operation name.
struct OpStats {
    count: u64,
    sum_seconds: f64,
    latency: LatencyHistogram,
}

impl OpStats {
    fn new() -> Self {
        Self {
            count: 0,
            sum_seconds: 0.0,
            latency: LatencyHistogram::new(),
        }
    }

    fn record(&mut self, elapsed: Duration) {
        self.count += 1;
        self.sum_seconds += elapsed.as_secs_f64();
        self.latency.record(elapsed);
    }
}

/// Cumulative counters read by the history sampler.
pub struct RawCounters {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub drive_read_ops: u64,
    pub drive_write_ops: u64,
    pub meta_ops: u64,
    pub http_total: u64,
    /// Requests by status class: index 0 = 2xx, 1 = 3xx, 2 = 4xx, 3 = 5xx.
    pub http_status: [u64; 4],
    pub read_latency: HistogramCounts,
    pub write_latency: HistogramCounts,
    pub http_latency: HistogramCounts,
}

pub struct ConsoleMetrics {
    started_at_ms: i64,
    storage_op_stats: Mutex<HashMap<String, OpStats>>,
    metadata_op_stats: Mutex<HashMap<String, OpStats>>,
    read_latency_histogram: LatencyHistogram,
    write_latency_histogram: LatencyHistogram,
    http_latency_histogram: LatencyHistogram,
    http_total: AtomicU64,
    http_status: [AtomicU64; 4],
    read_bytes_total: AtomicU64,
    write_bytes_total: AtomicU64,
    drive_read_total: AtomicU64,
    drive_write_total: AtomicU64,
    meta_total: AtomicU64,
    read_latency: RollingLatencyWindow,
    write_latency: RollingLatencyWindow,
    read_throughput: RollingBytesWindow,
    write_throughput: RollingBytesWindow,
    drive_read_ops: RollingCountWindow,
    drive_write_ops: RollingCountWindow,
    meta_ops: RollingCountWindow,
    active_s3_requests: AtomicUsize,
    process_cpu: ProcessCpuTracker,
}

impl ConsoleMetrics {
    pub fn new() -> Self {
        let window = Duration::from_secs(LATENCY_WINDOW_SECS);
        Self {
            started_at_ms: chrono::Utc::now().timestamp_millis(),
            storage_op_stats: Mutex::new(HashMap::new()),
            metadata_op_stats: Mutex::new(HashMap::new()),
            read_latency_histogram: LatencyHistogram::new(),
            write_latency_histogram: LatencyHistogram::new(),
            http_latency_histogram: LatencyHistogram::new(),
            http_total: AtomicU64::new(0),
            http_status: std::array::from_fn(|_| AtomicU64::new(0)),
            read_bytes_total: AtomicU64::new(0),
            write_bytes_total: AtomicU64::new(0),
            drive_read_total: AtomicU64::new(0),
            drive_write_total: AtomicU64::new(0),
            meta_total: AtomicU64::new(0),
            read_latency: RollingLatencyWindow::new(window),
            write_latency: RollingLatencyWindow::new(window),
            read_throughput: RollingBytesWindow::new(window),
            write_throughput: RollingBytesWindow::new(window),
            drive_read_ops: RollingCountWindow::new(window),
            drive_write_ops: RollingCountWindow::new(window),
            meta_ops: RollingCountWindow::new(window),
            active_s3_requests: AtomicUsize::new(0),
            process_cpu: ProcessCpuTracker::new(),
        }
    }

    pub fn begin_s3_request(&self) {
        self.active_s3_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn end_s3_request(&self) {
        self.active_s3_requests.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn update_process_cpu(&self, prom: &PrometheusMetrics) {
        let uptime = prom.uptime_seconds();
        if let Some(raw) = read_raw_process_metrics() {
            let ratio = self.process_cpu.usage_ratio(raw.cpu_seconds_total, uptime);
            prom.set_process_cpu_usage(ratio);
        }
    }

    pub fn record_storage_op(&self, operation: &str, elapsed: Duration, bytes: u64) {
        self.storage_op_stats
            .lock()
            .unwrap()
            .entry(operation.to_string())
            .or_insert_with(OpStats::new)
            .record(elapsed);
        if READ_LATENCY_OPS.contains(&operation) {
            self.read_latency.record(elapsed);
            self.read_latency_histogram.record(elapsed);
        } else if WRITE_LATENCY_OPS.contains(&operation) {
            self.write_latency.record(elapsed);
            self.write_latency_histogram.record(elapsed);
        }
        if READ_THROUGHPUT_OPS.contains(&operation) {
            self.read_throughput.record(bytes);
            self.read_bytes_total.fetch_add(bytes, Ordering::Relaxed);
        } else if WRITE_THROUGHPUT_OPS.contains(&operation) {
            self.write_throughput.record(bytes);
            self.write_bytes_total.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    pub fn record_drive_read_op(&self) {
        self.drive_read_ops.record();
        self.drive_read_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_drive_write_op(&self) {
        self.drive_write_ops.record();
        self.drive_write_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_metadata_op(&self, operation: &str, elapsed: Duration) {
        self.metadata_op_stats
            .lock()
            .unwrap()
            .entry(operation.to_string())
            .or_insert_with(OpStats::new)
            .record(elapsed);
        self.meta_ops.record();
        self.meta_total.fetch_add(1, Ordering::Relaxed);
    }

    /// `status_code` is the numeric HTTP status; codes outside 200-599 only count toward the total.
    pub fn record_http(&self, status_code: u16, elapsed: Duration) {
        self.http_total.fetch_add(1, Ordering::Relaxed);
        if let Some(class) = (status_code / 100).checked_sub(2).filter(|c| *c < 4) {
            self.http_status[class as usize].fetch_add(1, Ordering::Relaxed);
        }
        self.http_latency_histogram.record(elapsed);
    }

    pub fn raw_counters(&self) -> RawCounters {
        RawCounters {
            read_bytes: self.read_bytes_total.load(Ordering::Relaxed),
            write_bytes: self.write_bytes_total.load(Ordering::Relaxed),
            drive_read_ops: self.drive_read_total.load(Ordering::Relaxed),
            drive_write_ops: self.drive_write_total.load(Ordering::Relaxed),
            meta_ops: self.meta_total.load(Ordering::Relaxed),
            http_total: self.http_total.load(Ordering::Relaxed),
            http_status: std::array::from_fn(|i| self.http_status[i].load(Ordering::Relaxed)),
            read_latency: self.read_latency_histogram.counts(),
            write_latency: self.write_latency_histogram.counts(),
            http_latency: self.http_latency_histogram.counts(),
        }
    }

    pub fn active_clients(&self) -> u64 {
        self.active_s3_requests.load(Ordering::Relaxed) as u64
    }

    pub fn snapshot(&self, prom: &PrometheusMetrics) -> MetricsSnapshot {
        let caches = cache_name::ALL
            .iter()
            .map(|id| prom.cache_snapshot(id))
            .collect();

        let storage_ops = {
            let stats = self.storage_op_stats.lock().unwrap();
            let mut ops: Vec<_> = stats
                .iter()
                .map(|(operation, op)| {
                    let percentiles = op.latency.counts().percentiles();
                    StorageOpSnapshot {
                        operation: operation.clone(),
                        count: op.count,
                        sum_seconds: op.sum_seconds,
                        p50_seconds: percentiles.map(|p| p.p50),
                        p95_seconds: percentiles.map(|p| p.p95),
                        p99_seconds: percentiles.map(|p| p.p99),
                    }
                })
                .collect();
            ops.sort_by(|a, b| a.operation.cmp(&b.operation));
            ops
        };

        let metadata_ops = {
            let stats = self.metadata_op_stats.lock().unwrap();
            let mut ops: Vec<_> = stats
                .iter()
                .map(|(operation, op)| {
                    let percentiles = op.latency.counts().percentiles();
                    MetadataOpSnapshot {
                        operation: operation.clone(),
                        count: op.count,
                        sum_seconds: op.sum_seconds,
                        p50_seconds: percentiles.map(|p| p.p50),
                        p95_seconds: percentiles.map(|p| p.p95),
                        p99_seconds: percentiles.map(|p| p.p99),
                    }
                })
                .collect();
            ops.sort_by(|a, b| a.operation.cmp(&b.operation));
            ops
        };

        let uptime_seconds = prom.uptime_seconds();
        let cpu_usage_percent = prom.process_cpu_usage_ratio() * 100.0;
        let process = ProcessCpuTracker::snapshot(cpu_usage_percent);

        let http = self
            .http_status
            .each_ref()
            .map(|c| c.load(Ordering::Relaxed));

        MetricsSnapshot {
            started_at: self.started_at_ms,
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds,
            http: HttpSnapshot {
                total_requests: self.http_total.load(Ordering::Relaxed),
                status_2xx: http[0],
                status_3xx: http[1],
                status_4xx: http[2],
                status_5xx: http[3],
                bytes_read: self.read_bytes_total.load(Ordering::Relaxed),
                bytes_written: self.write_bytes_total.load(Ordering::Relaxed),
            },
            latency_percentiles: LatencyPercentilesSnapshot::default(),
            buckets: Vec::new(),
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
            active_clients: self.active_clients(),
            caches,
            storage_ops,
            metadata_ops,
            process,
        }
    }
}
