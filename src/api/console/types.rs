use axum::{
    Json,
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tokio_util::io::ReaderStream;

use crate::storage::{ByteStream, ObjectMeta, StorageError};

use super::error::{ConsoleError, object_not_found_response, version_not_found_response};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectGetOp {
    Metadata,
    Download,
    Presign { expires_secs: u64 },
    DownloadVersion { version_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectDeleteOp {
    Current,
    Version { version_id: String },
}

#[derive(Debug, Clone)]
pub struct PresignResult {
    pub url: String,
    pub expires_secs: u64,
}

pub enum ObjectGetResult {
    Metadata(ObjectMeta),
    Attachment {
        reader: ByteStream,
        meta: ObjectMeta,
    },
    Presign(PresignResult),
}

#[derive(serde::Deserialize, Default)]
pub struct ObjectGetQuery {
    pub download: Option<String>,
    pub presign: Option<String>,
    pub expires: Option<u64>,
    #[serde(rename = "versionId")]
    pub version_id: Option<String>,
}

#[derive(serde::Deserialize, Default)]
pub struct ObjectDeleteQuery {
    #[serde(rename = "versionId")]
    pub version_id: Option<String>,
}

fn query_flag(v: Option<&String>) -> bool {
    v.is_some_and(|s| !s.is_empty() && s != "0" && s != "false")
}

impl ObjectGetOp {
    pub fn from_query(params: &ObjectGetQuery) -> Result<Self, ConsoleError> {
        let download = query_flag(params.download.as_ref());
        let presign = query_flag(params.presign.as_ref());
        let version_id = params
            .version_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if presign {
            let expires_secs = params.expires.unwrap_or(3600).min(604_800);
            return Ok(Self::Presign { expires_secs });
        }
        if let Some(version_id) = version_id {
            if download {
                return Ok(Self::DownloadVersion { version_id });
            }
            return Err(ConsoleError::BadRequest(
                "versionId requires download=1".into(),
            ));
        }
        if download {
            return Ok(Self::Download);
        }
        Ok(Self::Metadata)
    }
}

impl ObjectDeleteOp {
    pub fn from_query(params: &ObjectDeleteQuery) -> Result<Self, ConsoleError> {
        if let Some(version_id) = params
            .version_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(Self::Version {
                version_id: version_id.to_string(),
            });
        }
        Ok(Self::Current)
    }
}

impl ObjectGetResult {
    pub fn into_response(self, key: &str) -> Response {
        match self {
            Self::Metadata(meta) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "key": meta.key,
                    "size": meta.size,
                    "lastModified": meta.last_modified,
                    "etag": meta.etag,
                    "contentType": meta.content_type,
                    "versionId": meta.version_id,
                    "isDeleteMarker": meta.is_delete_marker,
                    "tags": meta.tags.clone().unwrap_or_default(),
                })),
            )
                .into_response(),
            Self::Attachment { reader, meta } => attachment_response(key, reader, &meta),
            Self::Presign(result) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "url": result.url,
                    "expiresIn": result.expires_secs,
                })),
            )
                .into_response(),
        }
    }
}

pub(crate) fn attachment_response(key: &str, reader: ByteStream, meta: &ObjectMeta) -> Response {
    let filename = key.rsplit('/').next().unwrap_or(key);
    let safe_filename = sanitize_filename(filename);
    let stream = ReaderStream::with_capacity(reader, 256 * 1024);
    let body = Body::from_stream(stream);

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

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
        .collect()
}

pub(crate) fn map_get_storage_error(err: StorageError) -> Response {
    match err {
        StorageError::NotFound(_) => object_not_found_response(),
        StorageError::VersionNotFound(_) => version_not_found_response(),
        other => ConsoleError::Storage(other).into_response(),
    }
}
