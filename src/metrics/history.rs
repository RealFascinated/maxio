use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

use serde::Serialize;

use super::console::RawCounters;
use super::histogram::HistogramCounts;
use super::snapshot::{LatencyPercentilesSnapshot, PERCENTILE_WINDOW_SECS};
use crate::db::DbPool;
use crate::db::repos;

pub const SAMPLE_INTERVAL_SECS: u64 = 10;
pub const TICKS_PER_MINUTE: usize = 6;
/// Retention of the in-memory ring: 6h of 10s samples (2160 points).
pub const RING_RETENTION_MS: i64 = 6 * 60 * 60 * 1000;
pub const DB_RETENTION_DAYS: i64 = 30;
const MINUTE_MS: i64 = 60_000;

/// One point of history. Rates are per second; percentiles are seconds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub ts_ms: i64,
    pub read_bps: f64,
    pub write_bps: f64,
    pub read_iops: f64,
    pub write_iops: f64,
    pub meta_iops: f64,
    pub req_per_sec: f64,
    pub req_2xx: f64,
    pub req_3xx: f64,
    pub req_4xx: f64,
    pub req_5xx: f64,
    pub active_clients: f64,
    pub read_lat_p50: Option<f64>,
    pub read_lat_p95: Option<f64>,
    pub read_lat_p99: Option<f64>,
    pub write_lat_p50: Option<f64>,
    pub write_lat_p95: Option<f64>,
    pub write_lat_p99: Option<f64>,
    pub http_lat_p50: Option<f64>,
    pub http_lat_p95: Option<f64>,
    pub http_lat_p99: Option<f64>,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: Option<f64>,
    pub open_fds: Option<f64>,
    pub storage_bytes: Option<f64>,
    pub stored_bytes: Option<f64>,
    pub object_count: Option<f64>,
    /// Hit rate (0-1) per cache id; caches without lookups in the period are absent.
    pub cache_hit_rates: HashMap<String, f64>,
}

/// Cumulative counters read at each sampler tick.
pub struct Counters {
    pub raw: RawCounters,
    /// `(cache id, hits, misses)`
    pub caches: Vec<(&'static str, u64, u64)>,
}

/// Instantaneous values captured at a sampler tick.
#[derive(Debug, Clone, Default)]
pub struct Gauges {
    pub active_clients: f64,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: Option<f64>,
    pub open_fds: Option<f64>,
    pub storage_bytes: Option<f64>,
    pub stored_bytes: Option<f64>,
    pub object_count: Option<f64>,
}

/// A 10s sample plus the histogram deltas needed to roll percentiles up correctly.
pub struct Tick {
    pub sample: Sample,
    pub read_latency: HistogramCounts,
    pub write_latency: HistogramCounts,
    pub http_latency: HistogramCounts,
    /// `(cache id, hits, misses)` during the tick.
    pub cache_deltas: Vec<(&'static str, u64, u64)>,
}

pub fn build_tick(
    previous: &Counters,
    current: &Counters,
    elapsed_secs: f64,
    ts_ms: i64,
    gauges: Gauges,
) -> Tick {
    let elapsed = elapsed_secs.max(0.001);
    let rate = |now: u64, before: u64| now.saturating_sub(before) as f64 / elapsed;
    let (prev, cur) = (&previous.raw, &current.raw);

    let read_latency = cur.read_latency.delta_since(&prev.read_latency);
    let write_latency = cur.write_latency.delta_since(&prev.write_latency);
    let http_latency = cur.http_latency.delta_since(&prev.http_latency);
    let (read_p, write_p, http_p) = (
        read_latency.percentiles(),
        write_latency.percentiles(),
        http_latency.percentiles(),
    );

    let cache_deltas: Vec<_> = current
        .caches
        .iter()
        .map(|(id, hits, misses)| {
            let (prev_hits, prev_misses) = previous
                .caches
                .iter()
                .find(|(prev_id, _, _)| prev_id == id)
                .map(|(_, h, m)| (*h, *m))
                .unwrap_or((0, 0));
            (
                *id,
                hits.saturating_sub(prev_hits),
                misses.saturating_sub(prev_misses),
            )
        })
        .collect();

    let sample = Sample {
        ts_ms,
        read_bps: rate(cur.read_bytes, prev.read_bytes),
        write_bps: rate(cur.write_bytes, prev.write_bytes),
        read_iops: rate(cur.drive_read_ops, prev.drive_read_ops),
        write_iops: rate(cur.drive_write_ops, prev.drive_write_ops),
        meta_iops: rate(cur.meta_ops, prev.meta_ops),
        req_per_sec: rate(cur.http_total, prev.http_total),
        req_2xx: rate(cur.http_status[0], prev.http_status[0]),
        req_3xx: rate(cur.http_status[1], prev.http_status[1]),
        req_4xx: rate(cur.http_status[2], prev.http_status[2]),
        req_5xx: rate(cur.http_status[3], prev.http_status[3]),
        active_clients: gauges.active_clients,
        read_lat_p50: read_p.map(|p| p.p50),
        read_lat_p95: read_p.map(|p| p.p95),
        read_lat_p99: read_p.map(|p| p.p99),
        write_lat_p50: write_p.map(|p| p.p50),
        write_lat_p95: write_p.map(|p| p.p95),
        write_lat_p99: write_p.map(|p| p.p99),
        http_lat_p50: http_p.map(|p| p.p50),
        http_lat_p95: http_p.map(|p| p.p95),
        http_lat_p99: http_p.map(|p| p.p99),
        cpu_percent: gauges.cpu_percent,
        rss_bytes: gauges.rss_bytes,
        open_fds: gauges.open_fds,
        storage_bytes: gauges.storage_bytes,
        stored_bytes: gauges.stored_bytes,
        object_count: gauges.object_count,
        cache_hit_rates: hit_rates(&cache_deltas),
    };

    Tick {
        sample,
        read_latency,
        write_latency,
        http_latency,
        cache_deltas,
    }
}

fn hit_rates(deltas: &[(&'static str, u64, u64)]) -> HashMap<String, f64> {
    deltas
        .iter()
        .filter(|(_, hits, misses)| hits + misses > 0)
        .map(|(id, hits, misses)| (id.to_string(), *hits as f64 / (hits + misses) as f64))
        .collect()
}

fn mean(group: &[&Sample], field: impl Fn(&Sample) -> f64) -> f64 {
    group.iter().map(|s| field(s)).sum::<f64>() / group.len() as f64
}

fn present(group: &[&Sample], field: impl Fn(&Sample) -> Option<f64>) -> Vec<f64> {
    group.iter().filter_map(|s| field(s)).collect()
}

fn mean_opt(group: &[&Sample], field: impl Fn(&Sample) -> Option<f64>) -> Option<f64> {
    let values = present(group, field);
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn max_opt(group: &[&Sample], field: impl Fn(&Sample) -> Option<f64>) -> Option<f64> {
    present(group, field).into_iter().reduce(f64::max)
}

/// Combine samples into one point: mean for rates and gauges (p50 included), max for p95/p99.
fn aggregate(ts_ms: i64, group: &[&Sample]) -> Sample {
    let mut cache_sums: HashMap<&str, (f64, usize)> = HashMap::new();
    for sample in group {
        for (id, rate) in &sample.cache_hit_rates {
            let entry = cache_sums.entry(id).or_insert((0.0, 0));
            entry.0 += rate;
            entry.1 += 1;
        }
    }

    Sample {
        ts_ms,
        read_bps: mean(group, |s| s.read_bps),
        write_bps: mean(group, |s| s.write_bps),
        read_iops: mean(group, |s| s.read_iops),
        write_iops: mean(group, |s| s.write_iops),
        meta_iops: mean(group, |s| s.meta_iops),
        req_per_sec: mean(group, |s| s.req_per_sec),
        req_2xx: mean(group, |s| s.req_2xx),
        req_3xx: mean(group, |s| s.req_3xx),
        req_4xx: mean(group, |s| s.req_4xx),
        req_5xx: mean(group, |s| s.req_5xx),
        active_clients: mean(group, |s| s.active_clients),
        read_lat_p50: mean_opt(group, |s| s.read_lat_p50),
        read_lat_p95: max_opt(group, |s| s.read_lat_p95),
        read_lat_p99: max_opt(group, |s| s.read_lat_p99),
        write_lat_p50: mean_opt(group, |s| s.write_lat_p50),
        write_lat_p95: max_opt(group, |s| s.write_lat_p95),
        write_lat_p99: max_opt(group, |s| s.write_lat_p99),
        http_lat_p50: mean_opt(group, |s| s.http_lat_p50),
        http_lat_p95: max_opt(group, |s| s.http_lat_p95),
        http_lat_p99: max_opt(group, |s| s.http_lat_p99),
        cpu_percent: mean_opt(group, |s| s.cpu_percent),
        rss_bytes: mean_opt(group, |s| s.rss_bytes),
        open_fds: mean_opt(group, |s| s.open_fds),
        storage_bytes: mean_opt(group, |s| s.storage_bytes),
        stored_bytes: mean_opt(group, |s| s.stored_bytes),
        object_count: mean_opt(group, |s| s.object_count),
        cache_hit_rates: cache_sums
            .into_iter()
            .map(|(id, (sum, count))| (id.to_string(), sum / count as f64))
            .collect(),
    }
}

/// Roll the last ticks (normally six 10s ticks) up into one minute point. Latency percentiles
/// come from the merged histogram deltas and cache hit rates from summed hits/misses, never from
/// averaging per-tick percentiles/ratios. The point is stamped with the minute of the newest tick.
pub fn rollup(ticks: &[Tick]) -> Option<Sample> {
    let newest = ticks.last()?;
    let group: Vec<&Sample> = ticks.iter().map(|t| &t.sample).collect();
    let mut point = aggregate(
        newest.sample.ts_ms.div_euclid(MINUTE_MS) * MINUTE_MS,
        &group,
    );

    let (mut read, mut write, mut http) = (
        HistogramCounts::default(),
        HistogramCounts::default(),
        HistogramCounts::default(),
    );
    let mut cache_totals: HashMap<&'static str, (u64, u64)> = HashMap::new();
    for tick in ticks {
        read.merge(&tick.read_latency);
        write.merge(&tick.write_latency);
        http.merge(&tick.http_latency);
        for (id, hits, misses) in &tick.cache_deltas {
            let entry = cache_totals.entry(id).or_default();
            entry.0 += hits;
            entry.1 += misses;
        }
    }
    let (read, write, http) = (read.percentiles(), write.percentiles(), http.percentiles());
    point.read_lat_p50 = read.map(|p| p.p50);
    point.read_lat_p95 = read.map(|p| p.p95);
    point.read_lat_p99 = read.map(|p| p.p99);
    point.write_lat_p50 = write.map(|p| p.p50);
    point.write_lat_p95 = write.map(|p| p.p95);
    point.write_lat_p99 = write.map(|p| p.p99);
    point.http_lat_p50 = http.map(|p| p.p50);
    point.http_lat_p95 = http.map(|p| p.p95);
    point.http_lat_p99 = http.map(|p| p.p99);
    point.cache_hit_rates = cache_totals
        .into_iter()
        .filter(|(_, (hits, misses))| hits + misses > 0)
        .map(|(id, (hits, misses))| (id.to_string(), hits as f64 / (hits + misses) as f64))
        .collect();
    Some(point)
}

/// Group time-ordered samples into `step_ms` buckets and aggregate each; points are stamped
/// with the bucket start.
pub fn downsample(samples: &[Sample], step_ms: i64) -> Vec<Sample> {
    let mut out = Vec::new();
    let mut group: Vec<&Sample> = Vec::new();
    let mut group_start = 0;
    for sample in samples {
        let start = sample.ts_ms.div_euclid(step_ms) * step_ms;
        if !group.is_empty() && start != group_start {
            out.push(aggregate(group_start, &group));
            group.clear();
        }
        group_start = start;
        group.push(sample);
    }
    if !group.is_empty() {
        out.push(aggregate(group_start, &group));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRange {
    Minutes15,
    Hour1,
    Hours6,
    Hours24,
    Days7,
    Days30,
}

impl HistoryRange {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "15m" => Self::Minutes15,
            "1h" => Self::Hour1,
            "6h" => Self::Hours6,
            "24h" => Self::Hours24,
            "7d" => Self::Days7,
            "30d" => Self::Days30,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minutes15 => "15m",
            Self::Hour1 => "1h",
            Self::Hours6 => "6h",
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
        }
    }

    pub fn duration_secs(self) -> i64 {
        match self {
            Self::Minutes15 => 15 * 60,
            Self::Hour1 => 60 * 60,
            Self::Hours6 => 6 * 60 * 60,
            Self::Hours24 => 24 * 60 * 60,
            Self::Days7 => 7 * 24 * 60 * 60,
            Self::Days30 => 30 * 24 * 60 * 60,
        }
    }

    pub fn resolution_secs(self) -> i64 {
        match self {
            Self::Minutes15 | Self::Hour1 => SAMPLE_INTERVAL_SECS as i64,
            Self::Hours6 | Self::Hours24 => 60,
            Self::Days7 => 300,
            Self::Days30 => 1800,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySeries {
    pub read_bps: Vec<Option<f64>>,
    pub write_bps: Vec<Option<f64>>,
    pub read_iops: Vec<Option<f64>>,
    pub write_iops: Vec<Option<f64>>,
    pub meta_iops: Vec<Option<f64>>,
    pub req_per_sec: Vec<Option<f64>>,
    #[serde(rename = "req2xx")]
    pub req_2xx: Vec<Option<f64>>,
    #[serde(rename = "req3xx")]
    pub req_3xx: Vec<Option<f64>>,
    #[serde(rename = "req4xx")]
    pub req_4xx: Vec<Option<f64>>,
    #[serde(rename = "req5xx")]
    pub req_5xx: Vec<Option<f64>>,
    pub active_clients: Vec<Option<f64>>,
    pub read_latency_p50: Vec<Option<f64>>,
    pub read_latency_p95: Vec<Option<f64>>,
    pub read_latency_p99: Vec<Option<f64>>,
    pub write_latency_p50: Vec<Option<f64>>,
    pub write_latency_p95: Vec<Option<f64>>,
    pub write_latency_p99: Vec<Option<f64>>,
    pub http_latency_p50: Vec<Option<f64>>,
    pub http_latency_p95: Vec<Option<f64>>,
    pub http_latency_p99: Vec<Option<f64>>,
    pub cpu_percent: Vec<Option<f64>>,
    pub rss_bytes: Vec<Option<f64>>,
    pub open_fds: Vec<Option<f64>>,
    pub storage_bytes: Vec<Option<f64>>,
    pub stored_bytes: Vec<Option<f64>>,
    pub object_count: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResponse {
    pub range: &'static str,
    pub resolution_seconds: i64,
    pub from: i64,
    pub to: i64,
    pub t: Vec<i64>,
    pub series: HistorySeries,
    pub cache_hit_rates: BTreeMap<String, Vec<Option<f64>>>,
}

impl HistoryResponse {
    pub fn from_samples(range: HistoryRange, from: i64, to: i64, samples: &[Sample]) -> Self {
        let column = |field: fn(&Sample) -> Option<f64>| -> Vec<Option<f64>> {
            samples.iter().map(field).collect()
        };

        let mut cache_hit_rates: BTreeMap<String, Vec<Option<f64>>> = BTreeMap::new();
        for sample in samples {
            for id in sample.cache_hit_rates.keys() {
                cache_hit_rates.entry(id.clone()).or_default();
            }
        }
        for (id, column) in cache_hit_rates.iter_mut() {
            *column = samples
                .iter()
                .map(|s| s.cache_hit_rates.get(id).copied())
                .collect();
        }

        Self {
            range: range.as_str(),
            resolution_seconds: range.resolution_secs(),
            from,
            to,
            t: samples.iter().map(|s| s.ts_ms).collect(),
            series: HistorySeries {
                read_bps: column(|s| Some(s.read_bps)),
                write_bps: column(|s| Some(s.write_bps)),
                read_iops: column(|s| Some(s.read_iops)),
                write_iops: column(|s| Some(s.write_iops)),
                meta_iops: column(|s| Some(s.meta_iops)),
                req_per_sec: column(|s| Some(s.req_per_sec)),
                req_2xx: column(|s| Some(s.req_2xx)),
                req_3xx: column(|s| Some(s.req_3xx)),
                req_4xx: column(|s| Some(s.req_4xx)),
                req_5xx: column(|s| Some(s.req_5xx)),
                active_clients: column(|s| Some(s.active_clients)),
                read_latency_p50: column(|s| s.read_lat_p50),
                read_latency_p95: column(|s| s.read_lat_p95),
                read_latency_p99: column(|s| s.read_lat_p99),
                write_latency_p50: column(|s| s.write_lat_p50),
                write_latency_p95: column(|s| s.write_lat_p95),
                write_latency_p99: column(|s| s.write_lat_p99),
                http_latency_p50: column(|s| s.http_lat_p50),
                http_latency_p95: column(|s| s.http_lat_p95),
                http_latency_p99: column(|s| s.http_lat_p99),
                cpu_percent: column(|s| s.cpu_percent),
                rss_bytes: column(|s| s.rss_bytes),
                open_fds: column(|s| s.open_fds),
                storage_bytes: column(|s| s.storage_bytes),
                stored_bytes: column(|s| s.stored_bytes),
                object_count: column(|s| s.object_count),
            },
            cache_hit_rates,
        }
    }
}

struct HistoryState {
    ring: VecDeque<Sample>,
    window_percentiles: LatencyPercentilesSnapshot,
}

/// In-memory history: the 10s ring (6h) and the latest 60s latency percentiles.
pub struct MetricsHistory {
    state: Mutex<HistoryState>,
}

impl MetricsHistory {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HistoryState {
                ring: VecDeque::new(),
                window_percentiles: LatencyPercentilesSnapshot::default(),
            }),
        }
    }

    pub fn push(&self, sample: Sample) {
        let mut state = self.state.lock().unwrap();
        let cutoff = sample.ts_ms - RING_RETENTION_MS;
        state.ring.push_back(sample);
        while state.ring.front().is_some_and(|s| s.ts_ms < cutoff) {
            state.ring.pop_front();
        }
    }

    /// Insert older points (minute rollups loaded from Postgres) ahead of live samples.
    pub fn seed(&self, samples: Vec<Sample>) {
        let mut state = self.state.lock().unwrap();
        state.ring.extend(samples);
        state.ring.make_contiguous().sort_by_key(|s| s.ts_ms);
    }

    pub fn set_window_percentiles(&self, percentiles: LatencyPercentilesSnapshot) {
        self.state.lock().unwrap().window_percentiles = percentiles;
    }

    pub fn window_percentiles(&self) -> LatencyPercentilesSnapshot {
        self.state.lock().unwrap().window_percentiles.clone()
    }

    fn ring_since(&self, from_ms: i64) -> Vec<Sample> {
        let state = self.state.lock().unwrap();
        state
            .ring
            .iter()
            .filter(|s| s.ts_ms >= from_ms)
            .cloned()
            .collect()
    }

    /// Build the history for `range`: 15m/1h from the 10s ring, 6h from the ring downsampled to
    /// 1 minute, 24h from Postgres minutes plus the in-memory tail, 7d/30d aggregated in SQL.
    /// Database failures degrade to the in-memory data.
    pub async fn query(&self, pool: &DbPool, range: HistoryRange, now_ms: i64) -> HistoryResponse {
        let from = now_ms - range.duration_secs() * 1000;
        let samples = match range {
            HistoryRange::Minutes15 | HistoryRange::Hour1 => self.ring_since(from),
            HistoryRange::Hours6 => downsample(&self.ring_since(from), MINUTE_MS),
            HistoryRange::Hours24 | HistoryRange::Days7 | HistoryRange::Days30 => {
                let mut rows =
                    repos::load_metrics_samples_aggregated(pool, from, range.resolution_secs())
                        .await
                        .unwrap_or_else(|e| {
                            tracing::warn!("loading metrics history failed: {e}");
                            Vec::new()
                        });
                if range == HistoryRange::Hours24 {
                    let newest_row = rows.last().map(|s| s.ts_ms).unwrap_or(i64::MIN);
                    let tail = downsample(&self.ring_since(from), MINUTE_MS);
                    rows.extend(tail.into_iter().filter(|s| s.ts_ms > newest_row));
                }
                rows
            }
        };
        HistoryResponse::from_samples(range, from, now_ms, &samples)
    }
}

/// Merge the newest ticks (the 60s window) into percentiles for `/api/metrics`.
pub fn window_percentiles(ticks: &[Tick]) -> LatencyPercentilesSnapshot {
    let (mut read, mut write, mut http) = (
        HistogramCounts::default(),
        HistogramCounts::default(),
        HistogramCounts::default(),
    );
    for tick in ticks {
        read.merge(&tick.read_latency);
        write.merge(&tick.write_latency);
        http.merge(&tick.http_latency);
    }
    LatencyPercentilesSnapshot {
        window_seconds: PERCENTILE_WINDOW_SECS,
        read: read.percentiles(),
        write: write.percentiles(),
        http: http.percentiles(),
    }
}
