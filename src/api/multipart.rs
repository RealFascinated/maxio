use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};

use crate::api::authz::{check_bucket_access, check_object_access};
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::{ChecksumAlgorithm, StorageError};
use crate::xml::{response::to_xml, types::*};

use super::object::{body_to_reader, extract_checksum};

const COMPLETE_BODY_MAX: usize = 1024 * 1024;
const DEFAULT_MAX_PARTS: usize = 1000;
const DEFAULT_MAX_UPLOADS: usize = 1000;

pub async fn create_multipart_upload(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
) -> Result<Response<Body>, S3Error> {
    ensure_bucket_exists(&state, &bucket).await?;
    check_object_access(&state, &principal, &bucket, &key, "s3:PutObject").await?;

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let checksum_algorithm = headers
        .get("x-amz-checksum-algorithm")
        .and_then(|v| v.to_str().ok())
        .and_then(ChecksumAlgorithm::from_header_str);

    let upload = state
        .storage
        .create_multipart_upload(&bucket, &key, content_type, checksum_algorithm)
        .await
        .map_err(map_storage_err)?;

    let xml = to_xml(&InitiateMultipartUploadResult {
        bucket,
        key,
        upload_id: upload.upload_id,
    })
    .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub async fn upload_part(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    ensure_bucket_exists(&state, &bucket).await?;

    let upload_id = params
        .get("uploadId")
        .ok_or_else(|| S3Error::invalid_argument("missing uploadId"))?;

    let upload = state
        .storage
        .get_multipart_upload(upload_id)
        .await
        .map_err(map_storage_err)?;
    if upload.bucket != bucket {
        return Err(S3Error::no_such_upload(upload_id));
    }

    check_object_access(&state, &principal, &bucket, &upload.key, "s3:PutObject").await?;

    let part_number = params
        .get("partNumber")
        .ok_or_else(|| S3Error::invalid_argument("missing partNumber"))?
        .parse::<u32>()
        .map_err(|_| S3Error::invalid_part("invalid part number"))?;

    let checksum = extract_checksum(&headers);
    let reader = body_to_reader(&headers, body).await?;
    let part = state
        .storage
        .upload_part(&bucket, upload_id, part_number, reader, checksum)
        .await
        .map_err(map_storage_err)?;

    let _ = key;
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("ETag", &part.etag);
    if let (Some(algo), Some(val)) = (&part.checksum_algorithm, &part.checksum_value) {
        builder = builder.header(algo.header_name(), val.as_str());
    }
    Ok(builder.body(Body::empty()).unwrap())
}

pub async fn complete_multipart_upload(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
    _headers: HeaderMap,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    ensure_bucket_exists(&state, &bucket).await?;
    check_object_access(&state, &principal, &bucket, &key, "s3:PutObject").await?;

    let upload_id = params
        .get("uploadId")
        .ok_or_else(|| S3Error::invalid_argument("missing uploadId"))?;

    let bytes = axum::body::to_bytes(body, COMPLETE_BODY_MAX)
        .await
        .map_err(S3Error::internal)?;
    let body_str = String::from_utf8_lossy(&bytes);
    let parts = parse_complete_parts(&body_str)?;

    let result = state
        .storage
        .complete_multipart_upload(&bucket, upload_id, &parts)
        .await
        .map_err(map_storage_err)?;

    let xml = to_xml(&CompleteMultipartUploadResult {
        location: format!("/{}/{}", bucket, key),
        bucket,
        key,
        etag: result.etag,
    })
    .map_err(S3Error::internal)?;

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml");
    if let (Some(algo), Some(val)) = (&result.checksum_algorithm, &result.checksum_value) {
        builder = builder.header(algo.header_name(), val.as_str());
    }
    Ok(builder.body(Body::from(xml)).unwrap())
}

pub async fn abort_multipart_upload(
    State(state): State<AppState>,
    Path((bucket, _key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    ensure_bucket_exists(&state, &bucket).await?;
    let upload_id = params
        .get("uploadId")
        .ok_or_else(|| S3Error::invalid_argument("missing uploadId"))?;

    let upload = state
        .storage
        .get_multipart_upload(upload_id)
        .await
        .map_err(map_storage_err)?;
    if upload.bucket != bucket {
        return Err(S3Error::no_such_upload(upload_id));
    }

    check_object_access(
        &state,
        &principal,
        &bucket,
        &upload.key,
        "s3:AbortMultipartUpload",
    )
    .await?;

    state
        .storage
        .abort_multipart_upload(&bucket, upload_id)
        .await
        .map_err(map_storage_err)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

pub async fn list_parts(
    State(state): State<AppState>,
    Path((bucket, _key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    ensure_bucket_exists(&state, &bucket).await?;
    let upload_id = params
        .get("uploadId")
        .ok_or_else(|| S3Error::invalid_argument("missing uploadId"))?;

    let upload = state
        .storage
        .get_multipart_upload(upload_id)
        .await
        .map_err(map_storage_err)?;
    if upload.bucket != bucket {
        return Err(S3Error::no_such_upload(upload_id));
    }

    check_object_access(
        &state,
        &principal,
        &bucket,
        &upload.key,
        "s3:ListMultipartUploadParts",
    )
    .await?;

    let part_number_marker = params
        .get("part-number-marker")
        .map(|s| s.parse::<u32>())
        .transpose()
        .map_err(|_| S3Error::invalid_argument("invalid part-number-marker"))?;
    let max_parts = params
        .get("max-parts")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_PARTS)
        .clamp(1, DEFAULT_MAX_PARTS);

    let (parts, is_truncated) = state
        .storage
        .list_parts(&bucket, upload_id, part_number_marker, max_parts)
        .await
        .map_err(map_storage_err)?;

    let next_part_number_marker = if is_truncated {
        parts.last().map(|p| p.part_number)
    } else {
        None
    };

    let xml = to_xml(&ListPartsResult {
        bucket,
        key: upload.key,
        upload_id: upload_id.clone(),
        part_number_marker,
        next_part_number_marker,
        max_parts: Some(max_parts as i32),
        is_truncated,
        parts: parts
            .into_iter()
            .map(|p| PartEntry {
                part_number: p.part_number,
                last_modified: p.last_modified,
                etag: p.etag,
                size: p.size,
            })
            .collect(),
    })
    .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub async fn list_multipart_uploads(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    ensure_bucket_exists(&state, &bucket).await?;
    check_bucket_access(&state, &principal, &bucket, "s3:ListBucketMultipartUploads").await?;

    let prefix = params.get("prefix").map(String::as_str);
    let key_marker = params.get("key-marker").cloned();
    let upload_id_marker = params.get("upload-id-marker").cloned();
    let max_uploads = params
        .get("max-uploads")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_UPLOADS)
        .clamp(1, DEFAULT_MAX_UPLOADS);

    let mut uploads = state
        .storage
        .list_multipart_uploads(&bucket, prefix)
        .await
        .map_err(map_storage_err)?;

    uploads.sort_by(|a, b| {
        (a.key.as_str(), a.upload_id.as_str()).cmp(&(b.key.as_str(), b.upload_id.as_str()))
    });

    if let Some(ref km) = key_marker {
        let um = upload_id_marker.as_deref().unwrap_or("");
        uploads.retain(|u| u.key > *km || (u.key == *km && u.upload_id.as_str() > um));
    }

    let is_truncated = uploads.len() > max_uploads;
    let page: Vec<_> = uploads.into_iter().take(max_uploads).collect();
    let (next_key_marker, next_upload_id_marker) = if is_truncated {
        page.last()
            .map(|u| (Some(u.key.clone()), Some(u.upload_id.clone())))
            .unwrap_or((None, None))
    } else {
        (None, None)
    };

    let xml = to_xml(&ListMultipartUploadsResult {
        bucket,
        key_marker,
        upload_id_marker,
        next_key_marker,
        next_upload_id_marker,
        prefix: prefix.map(str::to_string),
        delimiter: params.get("delimiter").cloned(),
        max_uploads: Some(max_uploads as i32),
        is_truncated,
        uploads: page
            .into_iter()
            .map(|u| MultipartUploadEntry {
                key: u.key,
                upload_id: u.upload_id,
                initiated: u.initiated,
            })
            .collect(),
    })
    .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub(super) async fn ensure_bucket_exists(state: &AppState, bucket: &str) -> Result<(), S3Error> {
    match state.storage.head_bucket(bucket).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(S3Error::no_such_bucket(bucket)),
        Err(e) => Err(S3Error::internal(e)),
    }
}

pub(super) fn map_storage_err(err: StorageError) -> S3Error {
    match err {
        StorageError::ChecksumMismatch(_) => S3Error::bad_checksum("x-amz-checksum"),
        StorageError::UploadNotFound(upload_id) => S3Error::no_such_upload(&upload_id),
        StorageError::InvalidKey(msg) if msg.contains("part too small") => {
            S3Error::entity_too_small()
        }
        StorageError::InvalidKey(msg)
            if msg.contains("part")
                || msg.contains("etag")
                || msg.contains("upload")
                || msg.contains("at least one") =>
        {
            S3Error::invalid_part(&msg)
        }
        StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
        _ => S3Error::internal(err),
    }
}

fn parse_complete_parts(xml: &str) -> Result<Vec<(u32, String)>, S3Error> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut parts = Vec::new();
    let mut in_part = false;
    let mut in_part_number = false;
    let mut in_etag = false;
    let mut part_number: Option<u32> = None;
    let mut etag: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => match e.name().as_ref() {
                b"Part" => {
                    in_part = true;
                    part_number = None;
                    etag = None;
                }
                b"PartNumber" if in_part => in_part_number = true,
                b"ETag" if in_part => in_etag = true,
                _ => {}
            },
            Ok(quick_xml::events::Event::Text(e)) => {
                if in_part_number {
                    let value = e
                        .decode()
                        .map_err(|_| S3Error::malformed_xml())?
                        .into_owned();
                    part_number = Some(
                        value
                            .parse::<u32>()
                            .map_err(|_| S3Error::invalid_part("invalid part number"))?,
                    );
                    in_part_number = false;
                } else if in_etag {
                    let value = e
                        .decode()
                        .map_err(|_| S3Error::malformed_xml())?
                        .into_owned();
                    let normalized = if value.starts_with('"') && value.ends_with('"') {
                        value
                    } else {
                        format!("\"{}\"", value)
                    };
                    etag = Some(normalized);
                    in_etag = false;
                }
            }
            Ok(quick_xml::events::Event::End(e)) => match e.name().as_ref() {
                b"PartNumber" => in_part_number = false,
                b"ETag" => in_etag = false,
                b"Part" => {
                    let n = part_number.ok_or_else(S3Error::malformed_xml)?;
                    let tag = etag.clone().ok_or_else(S3Error::malformed_xml)?;
                    parts.push((n, tag));
                    in_part = false;
                }
                _ => {}
            },
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return Err(S3Error::malformed_xml()),
            _ => {}
        }
    }

    Ok(parts)
}
