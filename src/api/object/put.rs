use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::Response,
};

use crate::api::authz::check_object_access;
use crate::api::{acl, multipart};
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::StorageError;

use super::checksum::{body_to_reader, extract_checksum};
use super::copy::{copy_object, upload_part_copy};
use super::tagging::{parse_amz_tagging_header, put_object_tagging, put_object_tags};

pub async fn put_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
    req: axum::extract::Request,
) -> Result<Response<Body>, S3Error> {
    let headers = req.headers().clone();
    let body = req.into_body();
    if params.contains_key("acl") {
        return acl::handle_object_put_acl(state, bucket, key, params, headers, body, principal)
            .await;
    }
    if params.contains_key("uploadId") && headers.contains_key("x-amz-copy-source") {
        return upload_part_copy(
            State(state),
            Path((bucket, key)),
            Query(params),
            Extension(principal),
            headers,
        )
        .await;
    }

    if headers.contains_key("x-amz-copy-source") {
        return copy_object(
            State(state),
            Path((bucket, key)),
            Extension(principal),
            headers,
        )
        .await;
    }

    if params.contains_key("tagging") {
        return put_object_tagging(
            State(state),
            Path((bucket, key)),
            Extension(principal),
            body,
        )
        .await;
    }

    if params.contains_key("uploadId") {
        return multipart::upload_part(
            State(state),
            Path((bucket, key)),
            Query(params),
            Extension(principal),
            headers,
            body,
        )
        .await;
    }

    check_object_access(&state, &principal, &bucket, &key, "s3:PutObject").await?;

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    let reader = body_to_reader(&headers, body).await?;

    let content_md5 = headers
        .get("content-md5")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let checksum = extract_checksum(&headers);
    let inline_tags = parse_amz_tagging_header(&headers)?;

    let content_length = headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let result = state
        .storage
        .put_object(
            &bucket,
            &key,
            content_type,
            reader,
            checksum,
            content_length,
        )
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_bucket(&bucket),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            StorageError::ChecksumMismatch(_) => S3Error::bad_checksum("x-amz-checksum"),
            _ => S3Error::internal(e),
        })?;

    if let Some(ref expected_md5) = content_md5 {
        use base64::Engine;
        let etag_hex = result.etag.trim_matches('"');
        let md5_bytes = hex::decode(etag_hex).map_err(S3Error::internal)?;
        let computed_md5 = base64::engine::general_purpose::STANDARD.encode(md5_bytes);
        if computed_md5 != *expected_md5 {
            let _ = state.storage.delete_object(&bucket, &key).await;
            return Err(S3Error::bad_digest());
        }
    }

    if let Some(tags) = inline_tags {
        put_object_tags(&state, &bucket, &key, tags).await?;
    }

    let has_acl_header = headers.contains_key("x-amz-acl")
        || headers
            .keys()
            .any(|k| k.as_str().starts_with("x-amz-grant-"));
    if has_acl_header {
        let (owner_id, owner_display_name) = if principal.is_root {
            (
                crate::iam::ROOT_CANONICAL_ID.to_string(),
                crate::iam::ROOT_DISPLAY_NAME.to_string(),
            )
        } else {
            (
                principal.canonical_id.clone(),
                principal.display_name.clone(),
            )
        };
        acl::apply_put_object_acl(
            &state,
            &bucket,
            &key,
            &headers,
            &owner_id,
            &owner_display_name,
        )
        .await?;
    }

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("ETag", &result.etag)
        .header("Content-Length", result.size.to_string());
    if let Some(vid) = &result.version_id {
        builder = builder.header("x-amz-version-id", vid.as_str());
    }
    if let (Some(algo), Some(val)) = (&result.checksum_algorithm, &result.checksum_value) {
        builder = builder.header(algo.header_name(), val.as_str());
    }
    Ok(builder.body(Body::empty()).unwrap())
}
