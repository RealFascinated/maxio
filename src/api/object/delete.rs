use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::Response,
};
use std::collections::HashMap;

use crate::api::authz::{check_bucket_access, check_object_access, get_principal};
use crate::api::multipart;
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::StorageError;

use super::tagging::delete_object_tagging;

pub async fn delete_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    if params.contains_key("tagging") {
        return delete_object_tagging(State(state), Path((bucket, key))).await;
    }

    if params.contains_key("uploadId") {
        return multipart::abort_multipart_upload(State(state), Path((bucket, key)), Query(params))
            .await;
    }

    check_object_access(&state, &principal, &bucket, &key, "s3:DeleteObject").await?;

    // Permanent version deletion
    if let Some(version_id) = params.get("versionId") {
        let deleted = state
            .storage
            .delete_object_version(&bucket, &key, version_id)
            .await
            .map_err(|e| match e {
                StorageError::NotFound(ref msg) if msg == &bucket => {
                    S3Error::no_such_bucket(&bucket)
                }
                StorageError::VersionNotFound(_) => S3Error::no_such_version(version_id),
                _ => S3Error::internal(e),
            })?;

        let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
        builder = builder.header("x-amz-version-id", version_id.as_str());
        if deleted.is_delete_marker {
            builder = builder.header("x-amz-delete-marker", "true");
        }
        return Ok(builder.body(Body::empty()).unwrap());
    }

    let result = state
        .storage
        .delete_object(&bucket, &key)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(ref msg) if msg == &bucket => S3Error::no_such_bucket(&bucket),
            _ => S3Error::internal(e),
        })?;

    let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
    if let Some(vid) = &result.version_id {
        builder = builder.header("x-amz-version-id", vid.as_str());
    }
    if result.is_delete_marker {
        builder = builder.header("x-amz-delete-marker", "true");
    }
    Ok(builder.body(Body::empty()).unwrap())
}

const DELETE_BODY_MAX: usize = 1024 * 1024;

pub(crate) fn parse_delete_objects_xml(
    bytes: &[u8],
) -> Result<Vec<crate::storage::BatchDeleteObject>, S3Error> {
    let body_str = String::from_utf8_lossy(bytes);
    let mut objects = Vec::new();
    let mut reader = quick_xml::Reader::from_str(&body_str);
    reader.config_mut().trim_text(true);
    let mut in_object = false;
    let mut in_key = false;
    let mut in_version_id = false;
    let mut current_key = String::new();
    let mut current_version_id: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => match e.name().as_ref() {
                b"Object" => {
                    in_object = true;
                    current_key.clear();
                    current_version_id = None;
                }
                b"Key" if in_object => in_key = true,
                b"VersionId" if in_object => in_version_id = true,
                _ => {}
            },
            Ok(quick_xml::events::Event::Text(e)) if in_key => {
                current_key = e.unescape().unwrap_or_default().into_owned();
                in_key = false;
            }
            Ok(quick_xml::events::Event::Text(e)) if in_version_id => {
                current_version_id = Some(e.unescape().unwrap_or_default().into_owned());
                in_version_id = false;
            }
            Ok(quick_xml::events::Event::End(e)) => match e.name().as_ref() {
                b"Key" => in_key = false,
                b"VersionId" => in_version_id = false,
                b"Object" => {
                    if !current_key.is_empty() {
                        objects.push(crate::storage::BatchDeleteObject {
                            key: current_key.clone(),
                            version_id: current_version_id.clone(),
                        });
                    }
                    in_object = false;
                }
                _ => {}
            },
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return Err(S3Error::malformed_xml()),
            _ => {}
        }
    }

    if objects.is_empty() {
        let mut in_key = false;
        let mut reader = quick_xml::Reader::from_str(&body_str);
        reader.config_mut().trim_text(true);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Start(e)) if e.name().as_ref() == b"Key" => {
                    in_key = true;
                }
                Ok(quick_xml::events::Event::Text(e)) if in_key => {
                    objects.push(crate::storage::BatchDeleteObject {
                        key: e.unescape().unwrap_or_default().into_owned(),
                        version_id: None,
                    });
                    in_key = false;
                }
                Ok(quick_xml::events::Event::End(e)) if e.name().as_ref() == b"Key" => {
                    in_key = false;
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => return Err(S3Error::malformed_xml()),
                _ => {}
            }
        }
    }

    Ok(objects)
}

/// Handle POST /{bucket}?delete — multi-object delete (DeleteObjects API).
pub async fn delete_objects(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    req: axum::extract::Request,
) -> Result<Response<Body>, S3Error> {
    let principal = get_principal(req.extensions());
    let body = req.into_body();
    multipart::ensure_bucket_exists(&state, &bucket).await?;
    check_bucket_access(&state, &principal, &bucket, "s3:DeleteObject").await?;

    let bytes = axum::body::to_bytes(body, DELETE_BODY_MAX)
        .await
        .map_err(|e| S3Error::internal(e))?;
    let objects = parse_delete_objects_xml(&bytes)?;

    let batch_results = state
        .storage
        .delete_objects_batch(&bucket, &objects)
        .await
        .map_err(|e| S3Error::internal(e))?;

    let mut deleted_xml = String::new();
    let mut error_xml = String::new();
    for (obj, delete_result) in batch_results {
        match delete_result {
            Ok(dr) => {
                let mut entry = format!(
                    "<Deleted><Key>{}</Key>",
                    quick_xml::escape::escape(&obj.key)
                );
                if let Some(vid) = &dr.version_id {
                    entry.push_str(&format!("<VersionId>{}</VersionId>", vid));
                }
                if dr.is_delete_marker {
                    entry.push_str("<DeleteMarker>true</DeleteMarker>");
                }
                entry.push_str("</Deleted>");
                deleted_xml.push_str(&entry);
            }
            Err(e) => {
                error_xml.push_str(&format!(
                    "<Error><Key>{}</Key><Code>InternalError</Code><Message>{}</Message></Error>",
                    quick_xml::escape::escape(&obj.key),
                    quick_xml::escape::escape(&e.to_string())
                ));
            }
        }
    }

    let response_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <DeleteResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{}{}</DeleteResult>",
        deleted_xml, error_xml
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .body(Body::from(response_xml))
        .unwrap())
}
