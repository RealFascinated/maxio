use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::MetricsRegistry;
use super::history::{
    DB_RETENTION_DAYS, Gauges, RING_RETENTION_MS, SAMPLE_INTERVAL_SECS, TICKS_PER_MINUTE, Tick,
    build_tick, rollup, window_percentiles,
};
use super::process::{cpu_cores, read_raw_process_metrics};
use crate::db::DbPool;
use crate::db::repos;
use crate::stats::BucketStatsCache;

const TICKS_PER_HOUR: u64 = TICKS_PER_MINUTE as u64 * 60;

/// Start the history sampler: seeds the ring from Postgres, then every 10s records a sample,
/// every minute persists a rollup, and every hour prunes rows older than 30 days.
pub fn spawn_sampler(
    metrics: Arc<MetricsRegistry>,
    stats: Arc<BucketStatsCache>,
    pool: Arc<DbPool>,
) {
    tokio::spawn(async move {
        let now_ms = chrono::Utc::now().timestamp_millis();
        match repos::load_metrics_samples_aggregated(&pool, now_ms - RING_RETENTION_MS, 60).await {
            Ok(samples) => metrics.history().seed(samples),
            Err(e) => tracing::warn!("seeding metrics history failed: {e}"),
        }

        let mut ticker = tokio::time::interval(Duration::from_secs(SAMPLE_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut previous = metrics.counters();
        let mut previous_at = Instant::now();
        let mut previous_cpu_seconds = read_raw_process_metrics().map(|p| p.cpu_seconds_total);
        let mut recent_ticks: VecDeque<Tick> = VecDeque::with_capacity(TICKS_PER_MINUTE);
        let mut tick_count: u64 = 0;
        ticker.tick().await;

        loop {
            ticker.tick().await;
            let now = Instant::now();
            let elapsed_secs = now.duration_since(previous_at).as_secs_f64();
            let current = metrics.counters();

            let process = read_raw_process_metrics();
            let cpu_percent = process
                .as_ref()
                .zip(previous_cpu_seconds)
                .map(|(raw, prev)| {
                    ((raw.cpu_seconds_total - prev) / elapsed_secs.max(0.001) / cpu_cores() * 100.0)
                        .max(0.0)
                });
            previous_cpu_seconds = process.as_ref().map(|p| p.cpu_seconds_total);

            let bucket_stats = stats.get_all();
            let has_stats = !bucket_stats.is_empty();
            let gauges = Gauges {
                active_clients: metrics.active_clients() as f64,
                cpu_percent,
                rss_bytes: process.as_ref().map(|p| p.resident_memory_bytes as f64),
                open_fds: process.as_ref().map(|p| p.open_fds as f64),
                storage_bytes: has_stats
                    .then(|| bucket_stats.iter().map(|s| s.size_bytes as f64).sum()),
                stored_bytes: has_stats
                    .then(|| bucket_stats.iter().map(|s| s.stored_bytes as f64).sum()),
                object_count: has_stats
                    .then(|| bucket_stats.iter().map(|s| s.object_count as f64).sum()),
            };

            let tick = build_tick(
                &previous,
                &current,
                elapsed_secs,
                chrono::Utc::now().timestamp_millis(),
                gauges,
            );
            previous = current;
            previous_at = now;
            tick_count += 1;

            metrics.history().push(tick.sample.clone());
            if recent_ticks.len() == TICKS_PER_MINUTE {
                recent_ticks.pop_front();
            }
            recent_ticks.push_back(tick);
            let window = recent_ticks.make_contiguous();
            metrics
                .history()
                .set_window_percentiles(window_percentiles(window));

            if tick_count.is_multiple_of(TICKS_PER_MINUTE as u64)
                && let Some(minute) = rollup(window)
                && let Err(e) = repos::insert_metrics_sample(&pool, &minute).await
            {
                tracing::warn!("persisting metrics sample failed: {e}");
            }

            if tick_count.is_multiple_of(TICKS_PER_HOUR) {
                let cutoff = chrono::Utc::now() - chrono::Duration::days(DB_RETENTION_DAYS);
                if let Err(e) = repos::prune_metrics_samples(&pool, cutoff).await {
                    tracing::warn!("pruning metrics samples failed: {e}");
                }
            }
        }
    });
}
