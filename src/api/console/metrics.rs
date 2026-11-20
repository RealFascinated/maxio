use axum::{
    Json,
    extract::{Extension, Query, State},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::metrics::BucketUsageSnapshot;
use crate::metrics::history::HistoryRange;
use crate::server::AppState;

use super::error::ConsoleError;
use super::session::ConsoleSession;

async fn visible_bucket_stats(
    state: &AppState,
    session: &ConsoleSession,
) -> Vec<crate::stats::BucketStat> {
    let all_stats = state.stats.get_all();
    if session.is_root {
        return all_stats;
    }
    let Ok(buckets) = state.storage.list_buckets().await else {
        return Vec::new();
    };
    let visible =
        crate::iam::authz::filter_buckets_by_access(state, &session.principal(), buckets).await;
    let visible_names: std::collections::HashSet<&str> =
        visible.iter().map(|b| b.name.as_str()).collect();
    all_stats
        .into_iter()
        .filter(|s| visible_names.contains(s.name.as_str()))
        .collect()
}

pub async fn get_metrics_api(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
) -> impl IntoResponse {
    let mut snapshot = state.metrics.snapshot();
    let mut stats = visible_bucket_stats(&state, &session).await;
    snapshot.storage_totals = crate::metrics::StorageTotalsSnapshot::from_bucket_stats(&stats);
    stats.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    snapshot.buckets = stats
        .into_iter()
        .map(|s| BucketUsageSnapshot {
            name: s.name,
            object_count: s.object_count,
            size_bytes: s.size_bytes,
        })
        .collect();
    Json(snapshot)
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    range: Option<String>,
}

/// Root-only (registered under the admin routes): history is server-wide.
pub async fn get_metrics_history_api(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, ConsoleError> {
    let range = query.range.as_deref().unwrap_or("1h");
    let range = HistoryRange::parse(range).ok_or_else(|| {
        ConsoleError::BadRequest("range must be one of 15m, 1h, 6h, 24h, 7d, 30d".to_string())
    })?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    Ok(Json(
        state
            .metrics
            .history()
            .query(&state.db_pool, range, now_ms)
            .await,
    ))
}
