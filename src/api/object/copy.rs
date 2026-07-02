use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};

use crate::api::authz::check_object_access;
use crate::api::multipart;
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::StorageError;
use crate::xml::{
    response::to_xml,
    types::{CopyObjectResult, CopyPartResult},
};

use crate::storage::checksum::extract_upload_checksum;

/// Parse the `x-amz-copy-source` header into (src_bucket, src_key).
fn parse_copy_source(headers: &HeaderMap) -> Result<(String, String), S3Error> {
    let copy_source = headers
        .get("x-amz-copy-source")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| S3Error::invalid_argument("missing x-amz-copy-source header"))?;

    let decoded = percent_encoding::percent_decode_str(copy_source)
        .decode_utf8()
        .map_err(|_| S3Error::invalid_argument("invalid x-amz-copy-source encoding"))?;
    let trimmed = decoded.trim_start_matches('/');
    let (src_bucket, src_key) = trimmed
        .split_once('/')
        .ok_or_else(|| S3Error::invalid_argument("invalid x-amz-copy-source format"))?;
    Ok((src_bucket.to_string(), src_key.to_string()))
}

/// Parse `x-amz-copy-source-range: bytes=start-end` into (start, end) inclusive.
fn parse_copy_source_range(
    headers: &HeaderMap,
    src_size: u64,
) -> Result<Option<(u64, u64)>, S3Error> {
    let header = match headers
        .get("x-amz-copy-source-range")
        .and_then(|v| v.to_str().ok())
    {
        Some(h) => h,
        None => return Ok(None),
    };
    let spec = header
        .strip_prefix("bytes=")
        .ok_or_else(|| S3Error::invalid_argument("invalid x-amz-copy-source-range format"))?;
    let (start_str, end_str) = spec
        .split_once('-')
        .ok_or_else(|| S3Error::invalid_argument("invalid x-amz-copy-source-range format"))?;
    let start: u64 = start_str
        .parse()
        .map_err(|_| S3Error::invalid_argument("invalid range start"))?;
    let end: u64 = end_str
        .parse()
        .map_err(|_| S3Error::invalid_argument("invalid range end"))?;
    if start > end || end >= src_size {
        return Err(S3Error::invalid_range());
    }
    Ok(Some((start, end)))
}

pub(super) async fn upload_part_copy(
    State(state): State<AppState>,
    Path((bucket, _key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
) -> Result<Response<Body>, S3Error> {
    let (src_bucket, src_key) = parse_copy_source(&headers)?;

    let upload_id = params
        .get("uploadId")
        .ok_or_else(|| S3Error::invalid_argument("missing uploadId"))?;
    let part_number = params
        .get("partNumber")
        .ok_or_else(|| S3Error::invalid_argument("missing partNumber"))?
        .parse::<u32>()
        .map_err(|_| S3Error::invalid_part("invalid part number"))?;

    multipart::ensure_bucket_exists(&state, &bucket).await?;

    let upload = state
        .storage
        .get_multipart_upload(upload_id)
        .await
        .map_err(multipart::map_storage_err)?;
    if upload.bucket != bucket {
        return Err(S3Error::no_such_upload(upload_id));
    }

    check_object_access(&state, &principal, &src_bucket, &src_key, "s3:GetObject").await?;
    check_object_access(&state, &principal, &bucket, &upload.key, "s3:PutObject").await?;

    // Get source metadata first to validate range before opening the file
    let src_meta = state
        .storage
        .head_object(&src_bucket, &src_key)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(&src_key),
            _ => S3Error::internal(e),
        })?;

    let range = parse_copy_source_range(&headers, src_meta.size)?;

    let reader = match range {
        None => {
            let (r, _) = state
                .storage
                .get_object(&src_bucket, &src_key)
                .await
                .map_err(|e| match e {
                    StorageError::NotFound(_) => S3Error::no_such_key(&src_key),
                    StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                    _ => S3Error::internal(e),
                })?;
            r
        }
        Some((start, end)) => {
            let (r, _) = state
                .storage
                .get_object_range(&src_bucket, &src_key, start, end - start + 1)
                .await
                .map_err(|e| match e {
                    StorageError::NotFound(_) => S3Error::no_such_key(&src_key),
                    StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                    _ => S3Error::internal(e),
                })?;
            r
        }
    };

    let checksum = extract_upload_checksum(&headers);
    let part = state
        .storage
        .upload_part(&bucket, upload_id, part_number, reader, checksum)
        .await
        .map_err(multipart::map_storage_err)?;

    let xml = to_xml(&CopyPartResult {
        etag: part.etag,
        last_modified: src_meta.last_modified,
    })
    .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub(super) async fn copy_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
) -> Result<Response<Body>, S3Error> {
    let (src_bucket, src_key) = parse_copy_source(&headers)?;
    let (src_bucket, src_key) = (src_bucket.as_str(), src_key.as_str());

    check_object_access(&state, &principal, src_bucket, src_key, "s3:GetObject").await?;
    check_object_access(&state, &principal, &bucket, &key, "s3:PutObject").await?;

    let (reader, src_meta) = state
        .storage
        .get_object(src_bucket, src_key)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(src_key),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            _ => S3Error::internal(e),
        })?;

    // Determine content-type based on metadata directive
    let directive = headers
        .get("x-amz-metadata-directive")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("COPY");

    let content_type = match directive {
        "COPY" => src_meta.content_type.clone(),
        "REPLACE" => headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string(),
        _ => {
            return Err(S3Error::invalid_argument(
                "invalid x-amz-metadata-directive",
            ));
        }
    };

    // Propagate source checksum algorithm so it's recomputed during copy
    let checksum = src_meta.checksum_algorithm.map(|algo| (algo, None));

    // Write destination
    let result = state
        .storage
        .put_object(&bucket, &key, &content_type, reader, checksum, None)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_bucket(&bucket),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            _ => S3Error::internal(e),
        })?;

    let xml = to_xml(&CopyObjectResult {
        etag: result.etag,
        last_modified: result.last_modified,
    })
    .map_err(S3Error::internal)?;

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml");
    if let Some(vid) = &result.version_id {
        builder = builder.header("x-amz-version-id", vid.as_str());
    }
    Ok(builder.body(Body::from(xml)).unwrap())
}
