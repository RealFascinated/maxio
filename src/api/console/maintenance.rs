use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use crate::db::repos::MetaBlobSource;
use crate::server::AppState;
use crate::storage::blob::BlobStorage;
use crate::storage::orphans::{self, OrphanMetaEntry};

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
