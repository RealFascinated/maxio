use serde::Serialize;

pub const LATENCY_WINDOW_SECS: u64 = 60;

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
