use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    response::Response,
};
use http::StatusCode;

use super::multipart;
use crate::api::authz::check_bucket_access;
use crate::db::repos::{ConsoleListSort, SortOrder};
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::traits::{DelimitedListPage, ListPage};
use crate::storage::{ObjectMeta, StorageError};
use crate::xml::{response::to_xml, types::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListApiVersion {
    V1,
    V2,
}

struct ListObjectsRequest {
    prefix: String,
    /// `None` = no delimiter; `Some("")` = flat listing (mc); `Some("/")` etc.
    delimiter: Option<String>,
    max_keys: usize,
    start_after: Option<String>,
    marker: Option<String>,
    continuation_token: Option<String>,
}

impl ListObjectsRequest {
    fn from_params(
        params: &HashMap<String, String>,
        version: ListApiVersion,
    ) -> Result<Self, S3Error> {
        let delimiter = match params.get("delimiter") {
            None => None,
            Some(d) if d.is_empty() => Some(String::new()),
            Some(d) => Some(d.clone()),
        };
        Ok(Self {
            prefix: params.get("prefix").cloned().unwrap_or_default(),
            delimiter,
            max_keys: parse_max_keys(params)?,
            start_after: params.get("start-after").cloned(),
            marker: params.get("marker").cloned(),
            continuation_token: if version == ListApiVersion::V2 {
                params.get("continuation-token").cloned()
            } else {
                None
            },
        })
    }

    fn effective_start(&self) -> Option<String> {
        self.continuation_token
            .as_ref()
            .and_then(|t| {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(t)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
            })
            .or_else(|| self.marker.clone())
            .or_else(|| self.start_after.clone())
    }

    fn uses_delimited_listing(&self) -> bool {
        self.delimiter.as_ref().is_some_and(|d| !d.is_empty())
    }
}

enum ListObjectsPage {
    Flat(ListPage),
    Delimited(DelimitedListPage),
}

pub async fn handle_bucket_get(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    tracing::debug!("GET /{} params={:?}", bucket, params);

    if params.contains_key("policy") {
        return super::bucket::get_bucket_policy(state, bucket, principal).await;
    }
    if params.contains_key("policy-status") {
        return super::bucket::get_bucket_policy_status(state, bucket, principal).await;
    }
    if params.contains_key("acl") {
        return super::acl::handle_bucket_get_acl(state, bucket, params, principal).await;
    }

    if params.contains_key("uploads") {
        return multipart::list_multipart_uploads(
            State(state),
            Path(bucket),
            Query(params),
            Extension(principal),
        )
        .await;
    }

    if params.contains_key("versioning") {
        check_bucket_access(&state, &principal, &bucket, "s3:GetBucketVersioning").await?;
        return super::bucket::get_bucket_versioning(state, bucket).await;
    }

    if params.contains_key("cors") {
        check_bucket_access(&state, &principal, &bucket, "s3:GetBucketCors").await?;
        return super::bucket::get_bucket_cors(state, bucket).await;
    }

    if params.contains_key("lifecycle") {
        check_bucket_access(&state, &principal, &bucket, "s3:GetLifecycleConfiguration").await?;
        return super::bucket::get_bucket_lifecycle(state, bucket).await;
    }

    if params.contains_key("versions") {
        check_bucket_access(&state, &principal, &bucket, "s3:ListBucketVersions").await?;
        return list_object_versions(state, bucket, params).await;
    }

    if params.contains_key("location") {
        check_bucket_access(&state, &principal, &bucket, "s3:GetBucketLocation").await?;
        tracing::debug!("GetBucketLocation for {}", bucket);
        match state.storage.head_bucket(&bucket).await {
            Ok(true) => {}
            Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
            Err(StorageError::InvalidKey(_)) => return Err(S3Error::no_such_bucket(&bucket)),
            Err(e) => return Err(S3Error::internal(e)),
        }
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <LocationConstraint></LocationConstraint>";
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/xml")
            .body(Body::from(xml))
            .unwrap());
    }

    if params.get("list-type").map(|v| v.as_str()) == Some("2") {
        list_objects(state, bucket, params, principal, ListApiVersion::V2).await
    } else {
        list_objects(state, bucket, params, principal, ListApiVersion::V1).await
    }
}

fn parse_max_keys(params: &HashMap<String, String>) -> Result<usize, S3Error> {
    match params.get("max-keys") {
        None => Ok(1000),
        Some(raw) => {
            let n: i64 = raw
                .parse()
                .map_err(|_| S3Error::invalid_argument("Invalid value for max-keys"))?;
            if n < 0 {
                return Err(S3Error::invalid_argument("Invalid value for max-keys"));
            }
            Ok((n as usize).min(1000))
        }
    }
}

async fn list_objects(
    state: AppState,
    bucket: String,
    params: HashMap<String, String>,
    principal: Principal,
    version: ListApiVersion,
) -> Result<Response<Body>, S3Error> {
    check_bucket_access(&state, &principal, &bucket, "s3:ListBucket").await?;
    let req = ListObjectsRequest::from_params(&params, version)?;
    let page = list_objects_core(&state, &bucket, &req).await?;
    let xml = match version {
        ListApiVersion::V1 => {
            to_xml(&list_page_to_xml_v1(&bucket, &req, page)).map_err(S3Error::internal)?
        }
        ListApiVersion::V2 => {
            to_xml(&list_page_to_xml_v2(&bucket, &req, page)).map_err(S3Error::internal)?
        }
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn list_objects_core(
    state: &AppState,
    bucket: &str,
    req: &ListObjectsRequest,
) -> Result<ListObjectsPage, S3Error> {
    let start = req.effective_start();
    if req.uses_delimited_listing() {
        let delimiter = req.delimiter.as_deref().unwrap_or("/");
        let page = state
            .storage
            .list_objects_delimited_page(
                bucket,
                &req.prefix,
                delimiter,
                start.as_deref(),
                req.max_keys,
                None,
                ConsoleListSort::Name,
                SortOrder::Asc,
            )
            .await
            .map_err(S3Error::internal)?;
        Ok(ListObjectsPage::Delimited(page))
    } else {
        let page = state
            .storage
            .list_objects_page(bucket, &req.prefix, start.as_deref(), req.max_keys, None)
            .await
            .map_err(S3Error::internal)?;
        Ok(ListObjectsPage::Flat(page))
    }
}

fn object_entry(obj: &ObjectMeta) -> ObjectEntry {
    ObjectEntry {
        key: obj.key.clone(),
        last_modified: obj.last_modified.clone(),
        etag: obj.etag.clone(),
        size: obj.size,
        storage_class: "STANDARD".to_string(),
    }
}

fn list_page_to_xml_v2(
    bucket: &str,
    req: &ListObjectsRequest,
    page: ListObjectsPage,
) -> ListBucketResult {
    let (contents, common_prefixes, is_truncated, next_continuation) = match page {
        ListObjectsPage::Flat(p) => (
            p.objects.iter().map(object_entry).collect::<Vec<_>>(),
            vec![],
            p.is_truncated,
            p.next_continuation,
        ),
        ListObjectsPage::Delimited(p) => {
            let prefixes: Vec<CommonPrefix> = p
                .prefixes
                .into_iter()
                .map(|prefix| CommonPrefix { prefix })
                .collect();
            (
                p.files.iter().map(object_entry).collect::<Vec<_>>(),
                prefixes,
                p.next_continuation.is_some(),
                p.next_continuation,
            )
        }
    };

    let next_token = next_continuation.map(|key| {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(key)
    });

    let delimiter = req.delimiter.as_ref().filter(|d| !d.is_empty()).cloned();

    ListBucketResult {
        name: bucket.to_string(),
        prefix: req.prefix.clone(),
        key_count: contents.len() as i32 + common_prefixes.len() as i32,
        max_keys: req.max_keys as i32,
        is_truncated,
        contents,
        common_prefixes,
        continuation_token: req.continuation_token.clone(),
        next_continuation_token: next_token,
        delimiter,
        start_after: req.start_after.clone(),
    }
}

fn list_page_to_xml_v1(
    bucket: &str,
    req: &ListObjectsRequest,
    page: ListObjectsPage,
) -> ListBucketResultV1 {
    let (contents, common_prefixes, is_truncated, next_marker) = match page {
        ListObjectsPage::Flat(p) => (
            p.objects.iter().map(object_entry).collect::<Vec<_>>(),
            vec![],
            p.is_truncated,
            p.next_continuation,
        ),
        ListObjectsPage::Delimited(p) => {
            let prefixes: Vec<CommonPrefix> = p
                .prefixes
                .into_iter()
                .map(|prefix| CommonPrefix { prefix })
                .collect();
            (
                p.files.iter().map(object_entry).collect::<Vec<_>>(),
                prefixes,
                p.next_continuation.is_some(),
                p.next_continuation,
            )
        }
    };

    let delimiter = req.delimiter.as_ref().filter(|d| !d.is_empty()).cloned();

    ListBucketResultV1 {
        name: bucket.to_string(),
        prefix: req.prefix.clone(),
        marker: req.marker.clone().unwrap_or_default(),
        next_marker,
        max_keys: req.max_keys as i32,
        is_truncated,
        contents,
        common_prefixes,
        delimiter,
    }
}

async fn list_object_versions(
    state: AppState,
    bucket: String,
    params: HashMap<String, String>,
) -> Result<Response<Body>, S3Error> {
    let prefix = params.get("prefix").cloned().unwrap_or_default();
    let max_keys = parse_max_keys(&params)?;
    let key_marker = params.get("key-marker").map(|s| s.as_str());
    let version_id_marker = params.get("version-id-marker").map(|s| s.as_str());

    let page = state
        .storage
        .list_object_versions_page(&bucket, &prefix, key_marker, version_id_marker, max_keys)
        .await
        .map_err(|e| S3Error::internal(e))?;

    let mut latest_per_key: HashMap<String, String> = HashMap::new();
    for v in &page.items {
        let vid = v.version_id.clone().unwrap_or_else(|| "null".to_string());
        latest_per_key.entry(v.key.clone()).or_insert(vid);
    }

    let mut versions = Vec::new();
    let mut delete_markers = Vec::new();

    for v in &page.items {
        let vid = v.version_id.as_deref().unwrap_or("null");
        let is_latest = latest_per_key
            .get(&v.key)
            .is_some_and(|latest| latest == vid);
        if v.is_delete_marker {
            delete_markers.push(DeleteMarkerEntry {
                key: v.key.clone(),
                version_id: vid.to_string(),
                is_latest,
                last_modified: v.last_modified.clone(),
            });
        } else {
            versions.push(VersionEntry {
                key: v.key.clone(),
                version_id: vid.to_string(),
                is_latest,
                last_modified: v.last_modified.clone(),
                etag: v.etag.clone(),
                size: v.size,
                storage_class: "STANDARD".to_string(),
            });
        }
    }

    let result = ListVersionsResult {
        name: bucket,
        prefix,
        key_marker: key_marker.unwrap_or("").to_string(),
        version_id_marker: version_id_marker.unwrap_or("").to_string(),
        max_keys: max_keys as i32,
        is_truncated: page.is_truncated,
        next_key_marker: page.next_key_marker,
        next_version_id_marker: page.next_version_id_marker,
        versions,
        delete_markers,
    };

    let xml = to_xml(&result).map_err(|e| S3Error::internal(e))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}
