use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use tokio_util::io::ReaderStream;

use crate::api::authz::check_object_access;
use crate::api::{acl, multipart};
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::StorageError;

use super::checksum::add_checksum_header;
use super::conditions::{
    ConditionalResult, check_conditions, not_modified_response, parse_range, to_http_date,
};
use super::tagging::get_object_tagging;

pub async fn get_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    if params.contains_key("acl") {
        return acl::handle_object_get_acl(state, bucket, key, params, principal).await;
    }
    if params.contains_key("tagging") {
        return get_object_tagging(State(state), Path((bucket, key)), Extension(principal)).await;
    }

    if params.contains_key("uploadId") {
        return multipart::list_parts(
            State(state),
            Path((bucket, key)),
            Query(params),
            Extension(principal),
        )
        .await;
    }

    check_object_access(&state, &principal, &bucket, &key, "s3:GetObject").await?;

    if let Some(part_num_str) = params.get("partNumber") {
        let part_num: u32 = part_num_str
            .parse()
            .map_err(|_| S3Error::invalid_argument("invalid partNumber"))?;
        let meta = state
            .storage
            .head_object(&bucket, &key)
            .await
            .map_err(|e| match e {
                StorageError::NotFound(_) => S3Error::no_such_key(&key),
                StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                _ => S3Error::internal(e),
            })?;
        let part_sizes = meta
            .part_sizes
            .as_ref()
            .ok_or_else(|| S3Error::invalid_argument("object is not a multipart upload"))?;
        let idx = (part_num as usize)
            .checked_sub(1)
            .ok_or_else(|| S3Error::invalid_argument("partNumber must be >= 1"))?;
        if idx >= part_sizes.len() {
            return Err(S3Error::invalid_argument("partNumber exceeds total parts"));
        }
        let offset: u64 = part_sizes[..idx].iter().sum();
        let length = part_sizes[idx];
        let total_parts = part_sizes.len();

        let reader = state
            .storage
            .open_range(&bucket, &key, &meta, offset, length)
            .await
            .map_err(|e| match e {
                StorageError::NotFound(_) => S3Error::no_such_key(&key),
                _ => S3Error::internal(e),
            })?;

        let stream = ReaderStream::with_capacity(reader, 256 * 1024);
        let body = Body::from_stream(stream);
        return Ok(Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Type", &meta.content_type)
            .header("Content-Length", length.to_string())
            .header(
                "Content-Range",
                format!("bytes {}-{}/{}", offset, offset + length - 1, meta.size),
            )
            .header("ETag", &meta.etag)
            .header("Last-Modified", to_http_date(&meta.last_modified))
            .header("x-amz-mp-parts-count", total_parts.to_string())
            .body(body)
            .unwrap());
    }

    let range_header = headers.get("range").and_then(|v| v.to_str().ok());

    if let Some(range_str) = range_header {
        let meta = state
            .storage
            .head_object(&bucket, &key)
            .await
            .map_err(|e| match e {
                StorageError::NotFound(_) => S3Error::no_such_key(&key),
                StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                _ => S3Error::internal(e),
            })?;

        // Evaluate conditional headers before streaming any bytes
        if let Some(result) = check_conditions(&headers, &meta) {
            return match result {
                ConditionalResult::NotModified => Ok(not_modified_response(&meta)),
                ConditionalResult::PreconditionFailed => Err(S3Error::precondition_failed()),
            };
        }

        match parse_range(range_str, meta.size) {
            Ok(Some((start, end))) => {
                let length = end - start + 1;
                let reader = state
                    .storage
                    .open_range(&bucket, &key, &meta, start, length)
                    .await
                    .map_err(|e| match e {
                        StorageError::NotFound(_) => S3Error::no_such_key(&key),
                        _ => S3Error::internal(e),
                    })?;

                let stream = ReaderStream::with_capacity(reader, 256 * 1024);
                let body = Body::from_stream(stream);

                return Ok(Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header("Content-Type", &meta.content_type)
                    .header("Content-Length", length.to_string())
                    .header(
                        "Content-Range",
                        format!("bytes {}-{}/{}", start, end, meta.size),
                    )
                    .header("Accept-Ranges", "bytes")
                    .header("ETag", &meta.etag)
                    .header("Last-Modified", to_http_date(&meta.last_modified))
                    .body(body)
                    .unwrap());
            }
            Ok(None) => {
                // Unparseable or multi-range — fall through to full 200
            }
            Err(()) => {
                return Err(S3Error::invalid_range());
            }
        }
    }

    let (reader, meta) = if let Some(version_id) = params.get("versionId") {
        state
            .storage
            .get_object_version(&bucket, &key, version_id)
            .await
            .map_err(|e| match e {
                StorageError::VersionNotFound(_) => S3Error::no_such_version(version_id),
                StorageError::NotFound(_) => S3Error::no_such_key(&key),
                StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                _ => S3Error::internal(e),
            })?
    } else {
        state
            .storage
            .get_object(&bucket, &key)
            .await
            .map_err(|e| match e {
                StorageError::NotFound(_) => S3Error::no_such_key(&key),
                StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                _ => S3Error::internal(e),
            })?
    };

    // Evaluate conditional headers before opening the stream
    if let Some(result) = check_conditions(&headers, &meta) {
        drop(reader);
        return match result {
            ConditionalResult::NotModified => Ok(not_modified_response(&meta)),
            ConditionalResult::PreconditionFailed => Err(S3Error::precondition_failed()),
        };
    }

    let stream = ReaderStream::with_capacity(reader, 256 * 1024);
    let body = Body::from_stream(stream);

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &meta.content_type)
        .header("Content-Length", meta.size.to_string())
        .header("Accept-Ranges", "bytes")
        .header("ETag", &meta.etag)
        .header("Last-Modified", to_http_date(&meta.last_modified));
    if let Some(vid) = &meta.version_id {
        builder = builder.header("x-amz-version-id", vid.as_str());
    }
    builder = add_checksum_header(builder, &meta);
    Ok(builder.body(body).unwrap())
}

pub async fn head_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    if params.contains_key("acl") {
        return acl::handle_object_get_acl(state, bucket, key, params, principal).await;
    }
    if params.contains_key("tagging") {
        return get_object_tagging(State(state), Path((bucket, key)), Extension(principal)).await;
    }

    check_object_access(&state, &principal, &bucket, &key, "s3:GetObject").await?;
    let meta = if let Some(version_id) = params.get("versionId") {
        state
            .storage
            .head_object_version(&bucket, &key, version_id)
            .await
            .map_err(|e| match e {
                StorageError::VersionNotFound(_) => S3Error::no_such_version(version_id),
                StorageError::NotFound(_) => S3Error::no_such_key(&key),
                StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                _ => S3Error::internal(e),
            })?
    } else {
        state
            .storage
            .head_object(&bucket, &key)
            .await
            .map_err(|e| match e {
                StorageError::NotFound(_) => S3Error::no_such_key(&key),
                StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
                _ => S3Error::internal(e),
            })?
    };

    if let Some(result) = check_conditions(&headers, &meta) {
        return match result {
            ConditionalResult::NotModified => Ok(not_modified_response(&meta)),
            ConditionalResult::PreconditionFailed => Err(S3Error::precondition_failed()),
        };
    }

    if let Some(part_num_str) = params.get("partNumber") {
        let part_num: u32 = part_num_str
            .parse()
            .map_err(|_| S3Error::invalid_argument("invalid partNumber"))?;
        let part_sizes = meta
            .part_sizes
            .as_ref()
            .ok_or_else(|| S3Error::invalid_argument("object is not a multipart upload"))?;
        let idx = (part_num as usize)
            .checked_sub(1)
            .ok_or_else(|| S3Error::invalid_argument("partNumber must be >= 1"))?;
        if idx >= part_sizes.len() {
            return Err(S3Error::invalid_argument("partNumber exceeds total parts"));
        }
        let offset: u64 = part_sizes[..idx].iter().sum();
        let length = part_sizes[idx];
        let total_parts = part_sizes.len();

        let mut builder = Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Type", &meta.content_type)
            .header("Content-Length", length.to_string())
            .header(
                "Content-Range",
                format!("bytes {}-{}/{}", offset, offset + length - 1, meta.size),
            )
            .header("ETag", &meta.etag)
            .header("Last-Modified", to_http_date(&meta.last_modified))
            .header("x-amz-mp-parts-count", total_parts.to_string());
        if let Some(vid) = &meta.version_id {
            builder = builder.header("x-amz-version-id", vid.as_str());
        }
        return Ok(builder.body(Body::empty()).unwrap());
    }

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &meta.content_type)
        .header("Content-Length", meta.size.to_string())
        .header("ETag", &meta.etag)
        .header("Last-Modified", to_http_date(&meta.last_modified))
        .header("Accept-Ranges", "bytes");
    if let Some(vid) = &meta.version_id {
        builder = builder.header("x-amz-version-id", vid.as_str());
    }
    builder = add_checksum_header(builder, &meta);
    Ok(builder.body(Body::empty()).unwrap())
}
