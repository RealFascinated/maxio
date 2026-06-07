use std::sync::{Arc, RwLock};
use std::time::Duration;

use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use diesel_async::RunQueryDsl;

use crate::db::DbPool;
use crate::metrics::MetricsRegistry;

#[derive(Debug, Clone, serde::Serialize)]
pub struct BucketStat {
    pub name: String,
    pub object_count: i64,
    pub size_bytes: i64,
}

pub struct BucketStatsCache {
    snapshot: RwLock<Vec<BucketStat>>,
}

impl BucketStatsCache {
    /// Create the cache and spawn the 60-second background refresh task.
    /// The task also drives the uptime gauge on every tick.
    pub fn new(db_pool: Arc<DbPool>, metrics: Arc<MetricsRegistry>) -> Arc<Self> {
        let cache = Arc::new(Self {
            snapshot: RwLock::new(Vec::new()),
        });
        let cache2 = Arc::clone(&cache);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let t = std::time::Instant::now();
                match fetch_bucket_stats(&db_pool).await {
                    Ok(stats) => {
                        let elapsed = t.elapsed();
                        tracing::debug!(
                            "bucket stats refreshed: {} bucket(s) in {:.1}ms",
                            stats.len(),
                            elapsed.as_secs_f64() * 1000.0,
                        );
                        *cache2.snapshot.write().unwrap() = stats;
                    }
                    Err(e) => {
                        tracing::warn!("bucket stats refresh failed: {}", e);
                    }
                }
                metrics.update_uptime();
            }
        });
        cache
    }

    /// Return a snapshot of all bucket stats (cheap clone behind a read lock).
    pub fn get_all(&self) -> Vec<BucketStat> {
        self.snapshot.read().unwrap().clone()
    }

    /// Look up stats for a single bucket by name.
    pub fn get(&self, bucket: &str) -> Option<BucketStat> {
        self.snapshot
            .read()
            .unwrap()
            .iter()
            .find(|s| s.name == bucket)
            .cloned()
    }
}

#[derive(diesel::QueryableByName)]
struct StatRow {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = BigInt)]
    object_count: i64,
    #[diesel(sql_type = BigInt)]
    size_bytes: i64,
}

async fn fetch_bucket_stats(pool: &DbPool) -> anyhow::Result<Vec<BucketStat>> {
    let mut conn = pool.get().await?;
    let rows: Vec<StatRow> = sql_query(
        "SELECT b.name, \
         COUNT(o.id)::bigint AS object_count, \
         COALESCE(SUM(o.size), 0)::bigint AS size_bytes \
         FROM buckets b \
         LEFT JOIN objects o \
             ON o.bucket_id = b.id \
             AND o.is_delete_marker = false \
             AND o.is_folder_marker = false \
         GROUP BY b.name \
         ORDER BY b.name",
    )
    .load(&mut conn)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| BucketStat {
            name: r.name,
            object_count: r.object_count,
            size_bytes: r.size_bytes,
        })
        .collect())
}
