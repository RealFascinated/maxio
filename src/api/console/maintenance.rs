use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use crate::config::MemoryCacheLimits;
use crate::db::repos::MetaBlobSource;
use crate::server::AppState;
use crate::storage::blob::BlobStorage;
use crate::storage::orphans::{self, OrphanMetaEntry};
use crate::storage::{MetadataStore, PgMetadataStore};

fn orphan_to_json(entry: &OrphanMetaEntry) -> serde_json::Value {
    match &entry.source {
        MetaBlobSource::Current => serde_json::json!({
            "bucket": entry.bucket,
            "key": entry.key,
        }),
        MetaBlobSource::Version(version_id) => serde_json::json!({
            "bucket": entry.bucket,
            "key": entry.key,
            "versionId": version_id,
        }),
    }
}

async fn scan_orphans(state: &AppState) -> Result<Vec<OrphanMetaEntry>, String> {
    let blobs = BlobStorage::new(&state.config.data_dir)
        .await
        .map_err(|e| e.to_string())?;
    orphans::scan_orphaned_meta(
        Arc::clone(&state.db_pool),
        &blobs,
        state.config.cache_dir.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

pub async fn scan_orphan_meta_api(State(state): State<AppState>) -> impl IntoResponse {
    match scan_orphans(&state).await {
        Ok(orphans) => {
            let list: Vec<_> = orphans.iter().map(orphan_to_json).collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "asyncMetaWrite": state.config.async_meta_write,
                    "count": list.len(),
                    "orphans": list,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

pub async fn repair_orphan_meta_api(State(state): State<AppState>) -> impl IntoResponse {
    let orphans = match scan_orphans(&state).await {
        Ok(orphans) => orphans,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    if orphans.is_empty() {
        return (StatusCode::OK, Json(serde_json::json!({ "removed": 0 }))).into_response();
    }

    let cache_limits = MemoryCacheLimits::from(state.config.as_ref());
    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(
        Arc::clone(&state.db_pool),
        cache_limits,
    ));

    match orphans::delete_orphaned_meta(meta.as_ref(), &orphans).await {
        Ok(removed) => {
            tracing::info!(removed, "repaired orphaned metadata via console");
            (
                StatusCode::OK,
                Json(serde_json::json!({ "removed": removed })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
