use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::server::AppState;

use super::access::console_object_check;
use super::objects::sanitize_filename;
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
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    // Filter to only versions matching this exact key
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

pub async fn delete_version(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, version_id, key)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:DeleteObjectVersion").await
    {
        return resp;
    }

    match state
        .storage
        .delete_object_version(&bucket, &key, &version_id)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn download_version(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, version_id, key)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:GetObjectVersion").await
    {
        return resp;
    }

    let (reader, meta) = match state
        .storage
        .get_object_version(&bucket, &key, &version_id)
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Version not found"})),
            )
                .into_response();
        }
    };

    let filename = key.rsplit('/').next().unwrap_or(&key);
    let safe_filename = sanitize_filename(filename);
    let stream = tokio_util::io::ReaderStream::with_capacity(reader, 256 * 1024);
    let body = axum::body::Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &meta.content_type)
        .header("Content-Length", meta.size.to_string())
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", safe_filename),
        )
        .body(body)
        .unwrap()
        .into_response()
}
