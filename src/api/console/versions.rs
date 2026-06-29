use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::server::AppState;

use super::access::console_object_check;
use super::error::storage_error_response;
use super::session::ConsoleSession;

#[derive(serde::Deserialize)]
pub struct ListVersionsParams {
    key: String,
}

pub async fn list_versions(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Query(params): Query<ListVersionsParams>,
) -> impl IntoResponse {
    if let Err(resp) = console_object_check(
        &state,
        &session,
        &bucket,
        &params.key,
        "s3:GetObjectVersion",
    )
    .await
    {
        return resp;
    }

    let all = match state
        .storage
        .list_object_versions(&bucket, &params.key)
        .await
    {
        Ok(v) => v,
        Err(e) => return storage_error_response(e),
    };

    let versions: Vec<serde_json::Value> = all
        .into_iter()
        .filter(|v| v.key == params.key)
        .map(|v| {
            serde_json::json!({
                "versionId": v.version_id,
                "lastModified": v.last_modified,
                "size": v.size,
                "etag": v.etag,
                "isDeleteMarker": v.is_delete_marker,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"versions": versions})),
    )
        .into_response()
}
