use std::time::Duration;

use maxio::metrics::RawCounters;
use maxio::metrics::histogram::{
    BOUND_COUNT, HistogramCounts, LatencyHistogram, MAX_BOUND_SECONDS, MIN_BOUND_SECONDS,
};
use maxio::metrics::history::{
    Counters, Gauges, HistoryRange, HistoryResponse, Sample, build_tick, downsample, rollup,
};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

fn histogram_with(slot: usize, count: u64) -> HistogramCounts {
    let mut counts = HistogramCounts::default();
    counts.0[slot] = count;
    counts
}

fn raw_counters(http_total: u64, read_latency: HistogramCounts) -> RawCounters {
    RawCounters {
        read_bytes: http_total * 1000,
        write_bytes: 0,
        drive_read_ops: http_total,
        drive_write_ops: 0,
        meta_ops: 0,
        http_total,
        http_status: [http_total, 0, 0, 0],
        read_latency,
        write_latency: HistogramCounts::default(),
        http_latency: HistogramCounts::default(),
    }
}

fn counters(raw: RawCounters, cache_hits: u64, cache_misses: u64) -> Counters {
    Counters {
        raw,
        caches: vec![("bucket", cache_hits, cache_misses)],
    }
}

#[test]
fn histogram_records_into_log_buckets() {
    let histogram = LatencyHistogram::new();
    histogram.record(Duration::from_micros(300));
    histogram.record(Duration::from_secs_f64(MAX_BOUND_SECONDS));
    histogram.record(Duration::from_secs(61));

    let counts = histogram.counts();
    assert_eq!(counts.0[0], 1);
    assert_eq!(counts.0[BOUND_COUNT - 1], 1);
    assert_eq!(counts.0[BOUND_COUNT], 1);
    assert_eq!(counts.total(), 3);
}

#[test]
fn histogram_quantile_interpolates_within_bucket() {
    let counts = histogram_with(0, 100);
    assert!((counts.quantile(0.5).unwrap() - MIN_BOUND_SECONDS / 2.0).abs() < 1e-9);
    assert!((counts.quantile(1.0).unwrap() - MIN_BOUND_SECONDS).abs() < 1e-9);

    let overflow = histogram_with(BOUND_COUNT, 10);
    assert_eq!(overflow.quantile(0.99), Some(MAX_BOUND_SECONDS));

    assert_eq!(HistogramCounts::default().quantile(0.5), None);
    assert!(HistogramCounts::default().percentiles().is_none());
}

#[test]
fn histogram_percentiles_follow_the_distribution() {
    let mut counts = histogram_with(2, 90);
    counts.merge(&histogram_with(10, 10));
    let p = counts.percentiles().unwrap();
    assert!(p.p50 < p.p95 && p.p95 <= p.p99);
    // The slow 10% lives in slot 10, so p95 and p99 must sit above slot 2.
    assert!(p.p95 > p.p50 * 10.0);
}

#[test]
fn histogram_delta_ignores_earlier_counts() {
    let earlier = histogram_with(3, 5);
    let mut later = earlier;
    later.0[3] += 2;
    later.0[4] += 1;
    let delta = later.delta_since(&earlier);
    assert_eq!(delta.0[3], 2);
    assert_eq!(delta.0[4], 1);
    assert_eq!(delta.total(), 3);
}

#[test]
fn build_tick_derives_rates_and_percentiles_from_deltas() {
    let previous = counters(raw_counters(100, histogram_with(2, 100)), 10, 10);
    let current = counters(raw_counters(150, histogram_with(2, 100)), 40, 10);
    let tick = build_tick(&previous, &current, 10.0, 5_000, Gauges::default());

    assert!((tick.sample.req_per_sec - 5.0).abs() < 1e-9);
    assert!((tick.sample.read_bps - 5_000.0).abs() < 1e-9);
    assert_eq!(tick.sample.read_lat_p50, None);
    assert_eq!(tick.sample.cache_hit_rates.get("bucket"), Some(&1.0));
}

#[test]
fn rollup_merges_histograms_instead_of_averaging_percentiles() {
    let mut raw = raw_counters(0, HistogramCounts::default());
    let mut ticks = Vec::new();
    let mut previous = counters(raw_counters(0, HistogramCounts::default()), 0, 0);
    // Five fast ticks of 1 request, one slow tick of 100 requests.
    for i in 0..6u64 {
        let mut latency = raw.read_latency;
        if i == 5 {
            latency.0[12] += 100;
        } else {
            latency.0[1] += 1;
        }
        raw = raw_counters(raw.http_total + 1, latency);
        let current = counters(raw_counters(raw.http_total, latency), i * 10, 0);
        ticks.push(build_tick(
            &previous,
            &current,
            10.0,
            120_000 + i as i64 * 10_000 + 3_000,
            Gauges::default(),
        ));
        previous = current;
    }

    let minute = rollup(&ticks).unwrap();
    assert_eq!(minute.ts_ms, 120_000);
    assert!((minute.req_per_sec - 0.1).abs() < 1e-9);

    let mut merged = HistogramCounts::default();
    for tick in &ticks {
        merged.merge(&tick.read_latency);
    }
    let expected = merged.percentiles().unwrap();
    assert_eq!(minute.read_lat_p50, Some(expected.p50));
    assert_eq!(minute.read_lat_p99, Some(expected.p99));

    // Averaging per-tick p99s would be dominated by the five fast ticks; the merged p99 is not.
    let per_tick_mean: f64 = ticks
        .iter()
        .map(|t| t.sample.read_lat_p99.unwrap())
        .sum::<f64>()
        / 6.0;
    assert!(minute.read_lat_p99.unwrap() > per_tick_mean);
    assert!(rollup(&[]).is_none());
}

#[test]
fn downsample_groups_by_step_and_keeps_max_tail_latency() {
    let sample = |ts_ms: i64, rate: f64, p95: Option<f64>| Sample {
        ts_ms,
        req_per_sec: rate,
        http_lat_p50: p95,
        http_lat_p95: p95,
        ..Default::default()
    };
    let samples = vec![
        sample(60_000, 2.0, Some(0.1)),
        sample(70_000, 4.0, Some(0.3)),
        sample(120_000, 10.0, None),
    ];
    let out = downsample(&samples, 60_000);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].ts_ms, 60_000);
    assert!((out[0].req_per_sec - 3.0).abs() < 1e-9);
    assert_eq!(out[0].http_lat_p95, Some(0.3));
    assert!((out[0].http_lat_p50.unwrap() - 0.2).abs() < 1e-9);
    assert_eq!(out[1].ts_ms, 120_000);
    assert_eq!(out[1].http_lat_p95, None);
}

#[test]
fn history_range_selects_resolution() {
    let resolutions: Vec<_> = ["15m", "1h", "6h", "24h", "7d", "30d"]
        .iter()
        .map(|r| HistoryRange::parse(r).unwrap().resolution_secs())
        .collect();
    assert_eq!(resolutions, vec![10, 10, 60, 60, 300, 1800]);
    assert_eq!(HistoryRange::parse("7d").unwrap().duration_secs(), 604_800);
    assert!(HistoryRange::parse("2h").is_none());
}

#[test]
fn history_response_is_columnar_with_null_gaps() {
    let samples = vec![
        Sample {
            ts_ms: 10_000,
            req_5xx: 1.0,
            cache_hit_rates: [("bucket".to_string(), 0.5)].into(),
            ..Default::default()
        },
        Sample {
            ts_ms: 20_000,
            read_lat_p50: Some(0.01),
            ..Default::default()
        },
    ];
    let response = HistoryResponse::from_samples(HistoryRange::Hour1, 0, 30_000, &samples);
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["range"], "1h");
    assert_eq!(json["resolutionSeconds"], 10);
    assert_eq!(json["t"], serde_json::json!([10_000, 20_000]));
    assert_eq!(json["series"]["req5xx"], serde_json::json!([1.0, 0.0]));
    assert_eq!(
        json["series"]["readLatencyP50"],
        serde_json::json!([null, 0.01])
    );
    assert_eq!(
        json["cacheHitRates"]["bucket"],
        serde_json::json!([0.5, null])
    );
}

#[tokio::test]
async fn metrics_samples_repo_inserts_aggregates_and_prunes() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let database_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    maxio::db::run_migrations(&database_url).await.unwrap();
    let pool = maxio::db::create_pool(&database_url, Default::default())
        .await
        .unwrap();

    let now_ms = chrono::Utc::now().timestamp_millis();
    let base = now_ms.div_euclid(3_600_000) * 3_600_000;
    for minute in 0..4i64 {
        let sample = Sample {
            ts_ms: base + minute * 60_000,
            req_per_sec: minute as f64,
            http_lat_p50: Some(minute as f64),
            http_lat_p95: Some(minute as f64),
            cache_hit_rates: [("bucket".to_string(), 0.25 * minute as f64)].into(),
            ..Default::default()
        };
        maxio::db::repos::insert_metrics_sample(&pool, &sample)
            .await
            .unwrap();
    }
    let old = Sample {
        ts_ms: now_ms - 40 * 86_400_000,
        ..Default::default()
    };
    maxio::db::repos::insert_metrics_sample(&pool, &old)
        .await
        .unwrap();

    let minutes = maxio::db::repos::load_metrics_samples_aggregated(&pool, base - 1, 60)
        .await
        .unwrap();
    assert_eq!(minutes.len(), 4);
    assert_eq!(minutes[2].ts_ms, base + 120_000);
    assert_eq!(minutes[2].http_lat_p95, Some(2.0));
    assert_eq!(minutes[3].cache_hit_rates.get("bucket"), Some(&0.75));
    assert_eq!(minutes[0].read_lat_p50, None);

    let coarse = maxio::db::repos::load_metrics_samples_aggregated(&pool, base - 1, 300)
        .await
        .unwrap();
    assert_eq!(coarse.len(), 1);
    assert!((coarse[0].req_per_sec - 1.5).abs() < 1e-9);
    assert_eq!(coarse[0].http_lat_p95, Some(3.0));
    assert!((coarse[0].http_lat_p50.unwrap() - 1.5).abs() < 1e-9);
    assert!((coarse[0].cache_hit_rates["bucket"] - 0.375).abs() < 1e-9);

    let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
    let pruned = maxio::db::repos::prune_metrics_samples(&pool, cutoff)
        .await
        .unwrap();
    assert_eq!(pruned, 1);
}
