use axum::{
    Json,
    extract::{Extension, State},
    response::IntoResponse,
};

use crate::server::AppState;

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
    snapshot.storage_totals = crate::metrics::StorageTotalsSnapshot::from_bucket_stats(
        &visible_bucket_stats(&state, &session).await,
    );
    Json(snapshot)
}
