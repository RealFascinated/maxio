use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Double, Nullable, Text, Timestamptz};
use diesel_async::RunQueryDsl;
use serde_json::Value as JsonValue;

use crate::db::DbPool;
use crate::db::schema::metrics_samples;
use crate::metrics::history::Sample;
use crate::storage::StorageError;

use super::{db_err, get_conn};

#[derive(Insertable)]
#[diesel(table_name = metrics_samples)]
struct NewMetricsSample {
    bucket_start: DateTime<Utc>,
    read_bps: f64,
    write_bps: f64,
    read_iops: f64,
    write_iops: f64,
    meta_iops: f64,
    req_per_sec: f64,
    req_2xx: f64,
    req_3xx: f64,
    req_4xx: f64,
    req_5xx: f64,
    active_clients: f64,
    read_lat_p50: Option<f64>,
    read_lat_p95: Option<f64>,
    read_lat_p99: Option<f64>,
    write_lat_p50: Option<f64>,
    write_lat_p95: Option<f64>,
    write_lat_p99: Option<f64>,
    http_lat_p50: Option<f64>,
    http_lat_p95: Option<f64>,
    http_lat_p99: Option<f64>,
    cpu_percent: Option<f64>,
    rss_bytes: Option<f64>,
    open_fds: Option<f64>,
    storage_bytes: Option<f64>,
    object_count: Option<f64>,
    cache_hit_rates: JsonValue,
}

impl From<&Sample> for NewMetricsSample {
    fn from(s: &Sample) -> Self {
        Self {
            bucket_start: DateTime::from_timestamp_millis(s.ts_ms).unwrap_or_default(),
            read_bps: s.read_bps,
            write_bps: s.write_bps,
            read_iops: s.read_iops,
            write_iops: s.write_iops,
            meta_iops: s.meta_iops,
            req_per_sec: s.req_per_sec,
            req_2xx: s.req_2xx,
            req_3xx: s.req_3xx,
            req_4xx: s.req_4xx,
            req_5xx: s.req_5xx,
            active_clients: s.active_clients,
            read_lat_p50: s.read_lat_p50,
            read_lat_p95: s.read_lat_p95,
            read_lat_p99: s.read_lat_p99,
            write_lat_p50: s.write_lat_p50,
            write_lat_p95: s.write_lat_p95,
            write_lat_p99: s.write_lat_p99,
            http_lat_p50: s.http_lat_p50,
            http_lat_p95: s.http_lat_p95,
            http_lat_p99: s.http_lat_p99,
            cpu_percent: s.cpu_percent,
            rss_bytes: s.rss_bytes,
            open_fds: s.open_fds,
            storage_bytes: s.storage_bytes,
            object_count: s.object_count,
            cache_hit_rates: serde_json::to_value(&s.cache_hit_rates).unwrap_or_default(),
        }
    }
}

#[derive(QueryableByName)]
struct AggregatedRow {
    #[diesel(sql_type = BigInt)]
    ts_ms: i64,
    #[diesel(sql_type = Double)]
    read_bps: f64,
    #[diesel(sql_type = Double)]
    write_bps: f64,
    #[diesel(sql_type = Double)]
    read_iops: f64,
    #[diesel(sql_type = Double)]
    write_iops: f64,
    #[diesel(sql_type = Double)]
    meta_iops: f64,
    #[diesel(sql_type = Double)]
    req_per_sec: f64,
    #[diesel(sql_type = Double)]
    req_2xx: f64,
    #[diesel(sql_type = Double)]
    req_3xx: f64,
    #[diesel(sql_type = Double)]
    req_4xx: f64,
    #[diesel(sql_type = Double)]
    req_5xx: f64,
    #[diesel(sql_type = Double)]
    active_clients: f64,
    #[diesel(sql_type = Nullable<Double>)]
    read_lat_p50: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    read_lat_p95: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    read_lat_p99: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    write_lat_p50: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    write_lat_p95: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    write_lat_p99: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    http_lat_p50: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    http_lat_p95: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    http_lat_p99: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    cpu_percent: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    rss_bytes: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    open_fds: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    storage_bytes: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    object_count: Option<f64>,
}

#[derive(QueryableByName)]
struct CacheRateRow {
    #[diesel(sql_type = BigInt)]
    ts_ms: i64,
    #[diesel(sql_type = Text)]
    cache_id: String,
    #[diesel(sql_type = Double)]
    hit_rate: f64,
}

impl From<AggregatedRow> for Sample {
    fn from(r: AggregatedRow) -> Self {
        Self {
            ts_ms: r.ts_ms,
            read_bps: r.read_bps,
            write_bps: r.write_bps,
            read_iops: r.read_iops,
            write_iops: r.write_iops,
            meta_iops: r.meta_iops,
            req_per_sec: r.req_per_sec,
            req_2xx: r.req_2xx,
            req_3xx: r.req_3xx,
            req_4xx: r.req_4xx,
            req_5xx: r.req_5xx,
            active_clients: r.active_clients,
            read_lat_p50: r.read_lat_p50,
            read_lat_p95: r.read_lat_p95,
            read_lat_p99: r.read_lat_p99,
            write_lat_p50: r.write_lat_p50,
            write_lat_p95: r.write_lat_p95,
            write_lat_p99: r.write_lat_p99,
            http_lat_p50: r.http_lat_p50,
            http_lat_p95: r.http_lat_p95,
            http_lat_p99: r.http_lat_p99,
            cpu_percent: r.cpu_percent,
            rss_bytes: r.rss_bytes,
            open_fds: r.open_fds,
            storage_bytes: r.storage_bytes,
            object_count: r.object_count,
            cache_hit_rates: HashMap::new(),
        }
    }
}

pub async fn insert_metrics_sample(pool: &DbPool, sample: &Sample) -> Result<(), StorageError> {
    let mut conn = get_conn(pool).await?;
    diesel::insert_into(metrics_samples::table)
        .values(NewMetricsSample::from(sample))
        .on_conflict_do_nothing()
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Delete minute rows older than `cutoff`; returns the number of rows removed.
pub async fn prune_metrics_samples(
    pool: &DbPool,
    cutoff: DateTime<Utc>,
) -> Result<usize, StorageError> {
    let mut conn = get_conn(pool).await?;
    diesel::delete(metrics_samples::table.filter(metrics_samples::bucket_start.lt(cutoff)))
        .execute(&mut conn)
        .await
        .map_err(db_err)
}

/// Minute rows from `from_ms` onward regrouped into `step_secs` buckets (60 returns them
/// unchanged): average for rates, gauges and p50, max for p95/p99. Ordered by time.
pub async fn load_metrics_samples_aggregated(
    pool: &DbPool,
    from_ms: i64,
    step_secs: i64,
) -> Result<Vec<Sample>, StorageError> {
    let mut conn = get_conn(pool).await?;
    let from = DateTime::from_timestamp_millis(from_ms).unwrap_or_default();
    let bucket = "(floor(extract(epoch FROM bucket_start) / $2) * $2 * 1000)::bigint";

    let rows: Vec<AggregatedRow> = sql_query(format!(
        "SELECT {bucket} AS ts_ms, \
         avg(read_bps) AS read_bps, avg(write_bps) AS write_bps, \
         avg(read_iops) AS read_iops, avg(write_iops) AS write_iops, avg(meta_iops) AS meta_iops, \
         avg(req_per_sec) AS req_per_sec, avg(req_2xx) AS req_2xx, avg(req_3xx) AS req_3xx, \
         avg(req_4xx) AS req_4xx, avg(req_5xx) AS req_5xx, avg(active_clients) AS active_clients, \
         avg(read_lat_p50) AS read_lat_p50, max(read_lat_p95) AS read_lat_p95, max(read_lat_p99) AS read_lat_p99, \
         avg(write_lat_p50) AS write_lat_p50, max(write_lat_p95) AS write_lat_p95, max(write_lat_p99) AS write_lat_p99, \
         avg(http_lat_p50) AS http_lat_p50, max(http_lat_p95) AS http_lat_p95, max(http_lat_p99) AS http_lat_p99, \
         avg(cpu_percent) AS cpu_percent, avg(rss_bytes) AS rss_bytes, avg(open_fds) AS open_fds, \
         avg(storage_bytes) AS storage_bytes, avg(object_count) AS object_count \
         FROM metrics_samples WHERE bucket_start >= $1 GROUP BY 1 ORDER BY 1"
    ))
    .bind::<Timestamptz, _>(from)
    .bind::<BigInt, _>(step_secs)
    .load(&mut conn)
    .await
    .map_err(db_err)?;

    let cache_rows: Vec<CacheRateRow> = sql_query(format!(
        "SELECT {bucket} AS ts_ms, rates.key AS cache_id, avg(rates.value::float8) AS hit_rate \
         FROM metrics_samples, jsonb_each_text(cache_hit_rates) AS rates \
         WHERE bucket_start >= $1 GROUP BY 1, 2"
    ))
    .bind::<Timestamptz, _>(from)
    .bind::<BigInt, _>(step_secs)
    .load(&mut conn)
    .await
    .map_err(db_err)?;

    let mut samples: Vec<Sample> = rows.into_iter().map(Sample::from).collect();
    let index: HashMap<i64, usize> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| (s.ts_ms, i))
        .collect();
    for row in cache_rows {
        if let Some(i) = index.get(&row.ts_ms) {
            samples[*i]
                .cache_hit_rates
                .insert(row.cache_id, row.hit_rate);
        }
    }
    Ok(samples)
}
