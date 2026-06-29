use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};

use crate::api::authz::check_object_access;
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::StorageError;
use crate::xml::{
    response::to_xml,
    types::{Tag, TagSet, Tagging},
};

const TAGGING_BODY_MAX: usize = 64 * 1024;

pub fn parse_amz_tagging_header(
    headers: &HeaderMap,
) -> Result<Option<HashMap<String, String>>, S3Error> {
    let Some(raw) = headers.get("x-amz-tagging").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };
    let mut tags = HashMap::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| S3Error::invalid_argument("invalid x-amz-tagging header"))?;
        let key = percent_encoding::percent_decode_str(key)
            .decode_utf8()
            .map_err(|_| S3Error::invalid_argument("invalid x-amz-tagging header"))?
            .into_owned();
        let value = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map_err(|_| S3Error::invalid_argument("invalid x-amz-tagging header"))?
            .into_owned();
        tags.insert(key, value);
    }
    validate_tags(&tags)?;
    Ok(Some(tags))
}

pub fn validate_tags(tags: &HashMap<String, String>) -> Result<(), S3Error> {
    if tags.len() > 10 {
        return Err(S3Error::invalid_argument(
            "Object tags cannot exceed 10 entries",
        ));
    }
    for (k, v) in tags {
        if k.len() > 128 {
            return Err(S3Error::invalid_argument(
                "Tag key exceeds maximum length of 128 characters",
            ));
        }
        if v.len() > 256 {
            return Err(S3Error::invalid_argument(
                "Tag value exceeds maximum length of 256 characters",
            ));
        }
    }
    Ok(())
}

pub(super) async fn get_object_tagging(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    check_object_access(&state, &principal, &bucket, &key, "s3:GetObjectTagging").await?;

    let tags = state
        .storage
        .get_object_tagging(&bucket, &key)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(&key),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            _ => S3Error::internal(e),
        })?;

    let mut tag_entries: Vec<Tag> = tags
        .into_iter()
        .map(|(k, v)| Tag { key: k, value: v })
        .collect();
    tag_entries.sort_by(|a, b| a.key.cmp(&b.key));

    let tagging = Tagging {
        tag_set: TagSet { tags: tag_entries },
    };
    let xml = to_xml(&tagging).map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub(super) async fn put_object_tagging(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Extension(principal): Extension<Principal>,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    check_object_access(&state, &principal, &bucket, &key, "s3:PutObjectTagging").await?;

    let tags = parse_tagging_body(body).await?;

    state
        .storage
        .put_object_tagging(&bucket, &key, tags)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(&key),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            _ => S3Error::internal(e),
        })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub(super) async fn put_object_tags(
    state: &AppState,
    bucket: &str,
    key: &str,
    tags: HashMap<String, String>,
) -> Result<(), S3Error> {
    validate_tags(&tags)?;
    state
        .storage
        .put_object_tagging(bucket, key, tags)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(key),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            _ => S3Error::internal(e),
        })
}

pub(super) async fn delete_object_tagging(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    check_object_access(&state, &principal, &bucket, &key, "s3:DeleteObjectTagging").await?;

    state
        .storage
        .delete_object_tagging(&bucket, &key)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(&key),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            _ => S3Error::internal(e),
        })?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

async fn parse_tagging_body(body: Body) -> Result<HashMap<String, String>, S3Error> {
    let bytes = axum::body::to_bytes(body, TAGGING_BODY_MAX)
        .await
        .map_err(|e| S3Error::internal(e))?;
    let body_str = String::from_utf8_lossy(&bytes);

    let mut tags = HashMap::new();
    let mut reader = quick_xml::Reader::from_str(&body_str);
    reader.config_mut().trim_text(true);
    let mut current_key: Option<String> = None;
    let mut in_key = false;
    let mut in_value = false;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => match e.name().as_ref() {
                b"Key" => in_key = true,
                b"Value" => in_value = true,
                _ => {}
            },
            Ok(quick_xml::events::Event::Text(e)) => {
                let text = e.decode().unwrap_or_default().into_owned();
                if in_key {
                    current_key = Some(text);
                    in_key = false;
                } else if in_value {
                    if let Some(k) = current_key.take() {
                        tags.insert(k, text);
                    }
                    in_value = false;
                }
            }
            Ok(quick_xml::events::Event::End(e)) => match e.name().as_ref() {
                b"Key" => in_key = false,
                b"Value" => in_value = false,
                _ => {}
            },
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return Err(S3Error::malformed_xml()),
            _ => {}
        }
    }

    validate_tags(&tags)?;
    Ok(tags)
}
