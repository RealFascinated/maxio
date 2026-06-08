use std::collections::BTreeSet;

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::TryStreamExt;
use hmac::Mac;
use sha2::{Digest, Sha256};

use crate::auth::signature_v4;
use crate::config::Config;
use crate::server::AppState;
use crate::storage::Storage;

use super::access::{console_bucket_check, console_object_check};
use super::session::ConsoleSession;

type HmacSha256 = hmac::Hmac<Sha256>;

const CONSOLE_LIST_PAGE_SIZE: usize = 200;
const CONSOLE_LIST_SCAN_BATCH: usize = 200;
const CONSOLE_SEARCH_MAX_LEN: usize = 256;

#[derive(serde::Deserialize)]
pub struct ListObjectsParams {
    prefix: Option<String>,
    delimiter: Option<String>,
    start_after: Option<String>,
    max_keys: Option<usize>,
    q: Option<String>,
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

/// Classify one object into a direct file or a collapsed folder prefix.
fn classify_list_entry(
    obj: &crate::storage::ObjectMeta,
    prefix: &str,
    delimiter: &str,
    prefix_set: &mut BTreeSet<String>,
) -> Option<serde_json::Value> {
    if obj.key.ends_with('/') {
        if obj.key != prefix {
            prefix_set.insert(obj.key.clone());
        }
        None
    } else {
        let suffix = &obj.key[prefix.len()..];
        if let Some(pos) = suffix.find(delimiter) {
            let common = format!("{}{}", prefix, &suffix[..pos + delimiter.len()]);
            if common != prefix {
                prefix_set.insert(common);
            }
            None
        } else {
            Some(console_list_file_json(obj))
        }
    }
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

    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    let prefix = params.prefix.unwrap_or_default();
    let delimiter = params.delimiter.unwrap_or_else(|| "/".to_string());
    let max_keys = params.max_keys.unwrap_or(CONSOLE_LIST_PAGE_SIZE).max(1);
    let search = params.q.as_deref().map(str::trim).filter(|s| !s.is_empty());

    if let Some(q) = search {
        if q.len() > CONSOLE_SEARCH_MAX_LEN {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Search query too long"})),
            )
                .into_response();
        }
    }

    let mut files = Vec::new();
    let mut prefix_set = BTreeSet::new();
    let mut cursor = params.start_after;
    let mut next_continuation_token = None;

    'scan: loop {
        let page = match state
            .storage
            .list_objects_page(
                &bucket,
                &prefix,
                cursor.as_deref(),
                CONSOLE_LIST_SCAN_BATCH,
                search,
            )
            .await
        {
            Ok(page) => page,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response();
            }
        };

        if page.objects.is_empty() {
            break;
        }

        for (i, obj) in page.objects.iter().enumerate() {
            if let Some(file) = classify_list_entry(obj, &prefix, &delimiter, &mut prefix_set) {
                files.push(file);
            }

            if files.len() + prefix_set.len() >= max_keys {
                let more_in_batch = i + 1 < page.objects.len();
                if more_in_batch || page.is_truncated {
                    next_continuation_token = Some(obj.key.clone());
                }
                break 'scan;
            }
        }

        if !page.is_truncated {
            break;
        }
        cursor = page.next_continuation;
    }

    let prefixes: Vec<String> = prefix_set.into_iter().collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "files": files,
            "prefixes": prefixes,
            "nextContinuationToken": next_continuation_token,
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

    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
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
        .put_object(&bucket, &key, content_type, Box::pin(reader), None)
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_object_api(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:DeleteObject").await
    {
        return resp;
    }

    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    match state.storage.delete_object(&bucket, &key).await {
        Ok(_) => {
            if let Err(e) = preserve_empty_parent_folder_after_object_delete(
                state.storage.as_ref(),
                &bucket,
                &key,
            )
            .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e})),
                )
                    .into_response();
            }
            (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) fn parent_folder_prefix_for_deleted_object(key: &str) -> Option<String> {
    if key.ends_with('/') {
        return None;
    }
    key.rfind('/')
        .map(|idx| key[..=idx].to_string())
        .filter(|prefix| !prefix.is_empty())
}

pub(crate) async fn preserve_empty_parent_folder_after_object_delete(
    storage: &dyn Storage,
    bucket: &str,
    key: &str,
) -> Result<(), String> {
    let Some(parent_prefix) = parent_folder_prefix_for_deleted_object(key) else {
        return Ok(());
    };

    let remaining = crate::storage::list_objects_all(storage, bucket, &parent_prefix)
        .await
        .map_err(|e| e.to_string())?;

    let parent_still_exists = remaining
        .iter()
        .any(|obj| obj.key == parent_prefix || obj.key.starts_with(&parent_prefix));
    if parent_still_exists {
        return Ok(());
    }

    storage
        .put_object(
            bucket,
            &parent_prefix,
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub async fn download_object(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = console_object_check(&state, &session, &bucket, &key, "s3:GetObject").await {
        return resp;
    }

    let (reader, meta) = match state.storage.get_object(&bucket, &key).await {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Object not found"})),
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

/// Sanitize a filename for use in Content-Disposition headers.
/// Removes characters that could enable header injection.
pub(crate) fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
        .collect()
}

/// Scheme + host for presigned URL signing (must match what clients send on GET).
fn presign_endpoint(headers: &HeaderMap, config: &Config) -> (String, String) {
    if let Some(base) = config.public_url.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(uri) = base.parse::<http::Uri>() {
            let scheme = uri.scheme_str().unwrap_or("https").to_string();
            if let Some(authority) = uri.authority() {
                return (
                    scheme.clone(),
                    normalize_presign_host(authority.as_str(), &scheme),
                );
            }
        }
    }

    let scheme = presign_scheme(headers).to_string();
    let raw_host = headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("host").and_then(|v| v.to_str().ok()))
        .unwrap_or("localhost:9000");

    let host =
        if config.allow_insecure_dev && matches!(raw_host, "localhost:5173" | "127.0.0.1:5173") {
            format!("127.0.0.1:{}", config.port)
        } else {
            raw_host.to_string()
        };

    (scheme.clone(), normalize_presign_host(&host, &scheme))
}

fn presign_scheme(headers: &HeaderMap) -> &'static str {
    if headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
    {
        "https"
    } else {
        "http"
    }
}

pub(crate) fn normalize_presign_host(host: &str, scheme: &str) -> String {
    let host = host.split(',').next().unwrap_or(host).trim();
    if scheme == "https" {
        host.trim_end_matches(":443").to_string()
    } else if scheme == "http" {
        host.trim_end_matches(":80").to_string()
    } else {
        host.to_string()
    }
}

#[derive(serde::Deserialize)]
pub struct PresignParams {
    expires: Option<u64>,
}

pub async fn presign_object(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<PresignParams>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(resp) = console_object_check(&state, &session, &bucket, &key, "s3:GetObject").await {
        return resp;
    }

    // Verify object exists
    match state.storage.head_object(&bucket, &key).await {
        Ok(_) => {}
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Object not found"})),
            )
                .into_response();
        }
    }

    let expires_secs = params.expires.unwrap_or(3600).min(604800);

    let (scheme, host) = presign_endpoint(&headers, &state.config);

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let region = "us-east-1";
    let access_key = &state.config.access_key;

    let credential = format!("{}/{}/{}/s3/aws4_request", access_key, date_stamp, region);

    const S3_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    let encode =
        |s: &str| -> String { percent_encoding::utf8_percent_encode(s, S3_ENCODE).to_string() };

    // URI-encode each path segment per AWS SigV4 spec. The bucket/key values
    // arrive decoded from Axum's Path extractor, so we must encode them for
    // both the canonical request and the presigned URL.
    let encoded_key: String = key
        .split('/')
        .map(|s| encode(s))
        .collect::<Vec<_>>()
        .join("/");
    let path = format!("/{}/{}", encode(&bucket), encoded_key);

    // Build query string params (sorted alphabetically, excluding Signature)
    let qs_params = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential", credential.clone()),
        ("X-Amz-Date", amz_date.clone()),
        ("X-Amz-Expires", expires_secs.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];

    let canonical_qs: String = qs_params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!("host:{}\n", host);
    let canonical_request = format!(
        "GET\n{}\n{}\n{}\nhost\nUNSIGNED-PAYLOAD",
        path, canonical_qs, canonical_headers
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, scope, canonical_hash
    );

    let signing_key =
        signature_v4::derive_signing_key(&state.config.secret_key, &date_stamp, region);

    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let presigned_url = format!(
        "{}://{}{}?{}&X-Amz-Signature={}",
        scheme, host, path, canonical_qs, signature
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "url": presigned_url,
            "expiresIn": expires_secs,
        })),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct CreateFolderRequest {
    name: String,
}

pub async fn create_folder(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<CreateFolderRequest>,
) -> impl IntoResponse {
    let name = body.name.trim().trim_matches('/');
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Folder name is required"})),
        )
            .into_response();
    }

    let key = format!("{}/", name);
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
        )
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
