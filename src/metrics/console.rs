use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::cache_name;
use super::process::{ProcessCpuTracker, read_raw_process_metrics};
use super::prometheus::PrometheusMetrics;
use super::rolling::{RollingBytesWindow, RollingCountWindow, RollingLatencyWindow};
use super::snapshot::{
    LATENCY_WINDOW_SECS, LatencySnapshot, MetadataOpSnapshot, MetricsSnapshot, OpsTotalsSnapshot,
    StorageOpSnapshot, StorageTotalsSnapshot, ThroughputSnapshot,
};

const READ_LATENCY_OPS: &[&str] = &["get_object", "get_object_range"];
const WRITE_LATENCY_OPS: &[&str] = &["put_object", "complete_multipart_upload"];
const READ_THROUGHPUT_OPS: &[&str] = &["get_object", "get_object_range"];
const WRITE_THROUGHPUT_OPS: &[&str] = &["put_object", "upload_part", "complete_multipart_upload"];

pub struct ConsoleMetrics {
    storage_op_stats: Mutex<HashMap<String, (u64, f64)>>,
    metadata_op_stats: Mutex<HashMap<String, (u64, f64)>>,
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
            storage_op_stats: Mutex::new(HashMap::new()),
            metadata_op_stats: Mutex::new(HashMap::new()),
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
        let secs = elapsed.as_secs_f64();
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
        let mut stats = self.metadata_op_stats.lock().unwrap();
        let entry = stats.entry(operation.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += secs;
        self.meta_ops.record();
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

        let uptime_seconds = prom.uptime_seconds();
        let cpu_usage_percent = prom.process_cpu_usage_ratio() * 100.0;
        let process = ProcessCpuTracker::snapshot(cpu_usage_percent);

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
}
