use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures::TryStreamExt;

use crate::server::AppState;

use super::access::{console_bucket_check, console_object_check};
use super::error::{ConsoleError, storage_error_response};
use super::service::{
    ConsoleService, PresignContext, folder_delete_stats, normalize_folder_prefix,
};
use super::session::{ConsoleSession, session_signing_credentials};
use super::types::{
    ObjectDeleteOp, ObjectDeleteQuery, ObjectGetOp, ObjectGetQuery, map_get_storage_error,
};

const CONSOLE_LIST_PAGE_SIZE: usize = 200;
const CONSOLE_SEARCH_MAX_LEN: usize = 256;

#[derive(serde::Deserialize)]
pub struct ListObjectsParams {
    prefix: Option<String>,
    delimiter: Option<String>,
    start_after: Option<String>,
    max_keys: Option<usize>,
    q: Option<String>,
    sort: Option<String>,
    order: Option<String>,
}

fn console_list_file_json(obj: &crate::storage::ObjectMeta) -> serde_json::Value {
    serde_json::json!({
        "key": obj.key,
        "size": obj.size,
        "lastModified": obj.last_modified,
        "etag": obj.etag,
        "contentType": obj.content_type,
    })
}

pub async fn list_objects(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Query(params): Query<ListObjectsParams>,
) -> impl IntoResponse {
    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:ListBucket").await {
        return resp;
    }

    let svc = ConsoleService {
        storage: state.storage.as_ref(),
    };
    if let Err(e) = svc.ensure_bucket(&bucket).await {
        return storage_error_response(e);
    }

    let prefix = params.prefix.unwrap_or_default();
    let delimiter = params.delimiter.unwrap_or_else(|| "/".to_string());
    let max_keys = params.max_keys.unwrap_or(CONSOLE_LIST_PAGE_SIZE).max(1);
    let search = params.q.as_deref().map(str::trim).filter(|s| !s.is_empty());

    if let Some(q) = search {
        if q.len() > CONSOLE_SEARCH_MAX_LEN {
            return ConsoleError::BadRequest("Search query too long".into()).into_response();
        }
    }

    let page = match state
        .storage
        .list_objects_delimited_page(
            &bucket,
            &prefix,
            &delimiter,
            params.start_after.as_deref(),
            max_keys,
            search,
            crate::db::repos::ConsoleListSort::parse(params.sort.as_deref()),
            crate::db::repos::SortOrder::parse(params.order.as_deref()),
        )
        .await
    {
        Ok(page) => page,
        Err(e) => return storage_error_response(e),
    };

    let files: Vec<serde_json::Value> = page.files.iter().map(console_list_file_json).collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "files": files,
            "prefixes": page.prefixes,
            "nextContinuationToken": page.next_continuation,
        })),
    )
        .into_response()
}

pub async fn upload_object(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> impl IntoResponse {
    if let Err(resp) = console_object_check(&state, &session, &bucket, &key, "s3:PutObject").await {
        return resp;
    }

    let svc = ConsoleService {
        storage: state.storage.as_ref(),
    };
    if let Err(e) = svc.ensure_bucket(&bucket).await {
        return storage_error_response(e);
    }

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    let stream = body.into_data_stream();
    let reader = tokio_util::io::StreamReader::new(
        stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
    );

    match state
        .storage
        .put_object(&bucket, &key, content_type, Box::pin(reader), None, None)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "etag": result.etag,
                "size": result.size,
            })),
        )
            .into_response(),
        Err(e) => storage_error_response(e),
    }
}

pub async fn get_object_handler(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<ObjectGetQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let op = match ObjectGetOp::from_query(&params) {
        Ok(op) => op,
        Err(e) => return e.into_response(),
    };
    let action = match &op {
        ObjectGetOp::Presign { .. } => "s3:GetObject",
        ObjectGetOp::DownloadVersion { .. } => "s3:GetObjectVersion",
        _ => "s3:GetObject",
    };
    if let Err(resp) = console_object_check(&state, &session, &bucket, &key, action).await {
        return resp;
    }

    let svc = ConsoleService {
        storage: state.storage.as_ref(),
    };
    if let Err(e) = svc.ensure_bucket(&bucket).await {
        return storage_error_response(e);
    }

    let presign = if matches!(op, ObjectGetOp::Presign { .. }) {
        let Some((access_key, secret_key)) = session_signing_credentials(&state, &session).await
        else {
            return ConsoleError::Unauthorized("Not authenticated".into()).into_response();
        };
        Some(PresignContext {
            headers: &headers,
            config: &state.config,
            access_key,
            secret_key,
        })
    } else {
        None
    };

    match svc.get_object(&bucket, &key, op, presign).await {
        Ok(result) => result.into_response(&key),
        Err(e) => map_get_storage_error(e),
    }
}

pub async fn delete_object_handler(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<ObjectDeleteQuery>,
) -> impl IntoResponse {
    let op = match ObjectDeleteOp::from_query(&params) {
        Ok(op) => op,
        Err(e) => return e.into_response(),
    };
    let action = match &op {
        ObjectDeleteOp::Version { .. } => "s3:DeleteObjectVersion",
        ObjectDeleteOp::Current => "s3:DeleteObject",
    };
    if let Err(resp) = console_object_check(&state, &session, &bucket, &key, action).await {
        return resp;
    }

    let svc = ConsoleService {
        storage: state.storage.as_ref(),
    };
    if let Err(e) = svc.ensure_bucket(&bucket).await {
        return storage_error_response(e);
    }

    match svc.delete_object(&bucket, &key, op).await {
        Ok(()) => {
            if let Err(e) = svc.preserve_parent_folder(&bucket, &key).await {
                return storage_error_response(e);
            }
            (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
        }
        Err(e) => storage_error_response(e),
    }
}

#[derive(serde::Deserialize)]
pub struct DeleteObjectsRequest {
    keys: Vec<String>,
}

pub async fn delete_objects_api(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<DeleteObjectsRequest>,
) -> impl IntoResponse {
    let keys: Vec<String> = body
        .keys
        .into_iter()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect();
    if keys.is_empty() {
        return ConsoleError::BadRequest("At least one object key is required".into())
            .into_response();
    }

    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:ListBucket").await {
        return resp;
    }
    for key in &keys {
        if let Err(resp) =
            console_object_check(&state, &session, &bucket, key, "s3:DeleteObject").await
        {
            return resp;
        }
    }

    let svc = ConsoleService {
        storage: state.storage.as_ref(),
    };
    if let Err(e) = svc.ensure_bucket(&bucket).await {
        return storage_error_response(e);
    }

    let outcome = match svc.batch_delete(&bucket, &keys).await {
        Ok(outcome) => outcome,
        Err(e) => return storage_error_response(e),
    };

    for key in &outcome.succeeded {
        if let Err(e) = svc.preserve_parent_folder(&bucket, key).await {
            return storage_error_response(e);
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "deleted": outcome.succeeded.len(),
            "failed": outcome.failed,
        })),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct CreateFolderRequest {
    name: String,
}

#[derive(serde::Deserialize)]
pub struct FolderPreviewRequest {
    names: Vec<String>,
}

pub async fn create_folder(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<CreateFolderRequest>,
) -> impl IntoResponse {
    let Some(key) = normalize_folder_prefix(&body.name) else {
        return ConsoleError::BadRequest("Folder name is required".into()).into_response();
    };

    if let Err(resp) = console_object_check(&state, &session, &bucket, &key, "s3:PutObject").await {
        return resp;
    }

    match state
        .storage
        .put_object(
            &bucket,
            &key,
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
            None,
        )
        .await
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => storage_error_response(e),
    }
}

pub async fn preview_folder_delete(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<FolderPreviewRequest>,
) -> impl IntoResponse {
    let prefixes: Vec<String> = body
        .names
        .iter()
        .filter_map(|name| normalize_folder_prefix(name))
        .collect();
    if prefixes.is_empty() {
        return ConsoleError::BadRequest("At least one folder name is required".into())
            .into_response();
    }

    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:ListBucket").await {
        return resp;
    }
    for prefix in &prefixes {
        if let Err(resp) =
            console_object_check(&state, &session, &bucket, prefix, "s3:DeleteObject").await
        {
            return resp;
        }
    }

    let svc = ConsoleService {
        storage: state.storage.as_ref(),
    };
    if let Err(e) = svc.ensure_bucket(&bucket).await {
        return storage_error_response(e);
    }

    match folder_delete_stats(state.storage.as_ref(), &bucket, &prefixes).await {
        Ok((count, size_bytes)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "count": count,
                "sizeBytes": size_bytes,
            })),
        )
            .into_response(),
        Err(e) => storage_error_response(e),
    }
}

pub async fn delete_folder(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<CreateFolderRequest>,
) -> impl IntoResponse {
    let Some(prefix) = normalize_folder_prefix(&body.name) else {
        return ConsoleError::BadRequest("Folder name is required".into()).into_response();
    };

    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:ListBucket").await {
        return resp;
    }
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &prefix, "s3:DeleteObject").await
    {
        return resp;
    }

    let svc = ConsoleService {
        storage: state.storage.as_ref(),
    };
    if let Err(e) = svc.ensure_bucket(&bucket).await {
        return storage_error_response(e);
    }

    let objects =
        match crate::storage::list_objects_all(state.storage.as_ref(), &bucket, &prefix).await {
            Ok(objects) => objects,
            Err(e) => return storage_error_response(e),
        };

    if objects.is_empty() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"ok": true, "deleted": 0})),
        )
            .into_response();
    }

    let keys: Vec<String> = objects.into_iter().map(|obj| obj.key).collect();
    let outcome = match svc.batch_delete(&bucket, &keys).await {
        Ok(outcome) => outcome,
        Err(e) => return storage_error_response(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "deleted": outcome.succeeded.len(),
        })),
    )
        .into_response()
}
