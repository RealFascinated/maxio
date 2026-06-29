use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::error::{S3Error, S3ErrorCode};
use crate::storage::StorageError;

#[derive(Debug)]
pub enum ConsoleError {
    Storage(StorageError),
    BadRequest(String),
    Unauthorized(String),
}

impl IntoResponse for ConsoleError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response(),
            Self::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response(),
            Self::Storage(err) => storage_error_response(err),
        }
    }
}

pub(crate) fn storage_error_response(err: StorageError) -> Response {
    match &err {
        StorageError::NotFound(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bucket not found"})),
        )
            .into_response(),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) fn s3_error_to_response(err: S3Error) -> Response {
    match err.code {
        S3ErrorCode::NoSuchBucket => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bucket not found"})),
        )
            .into_response(),
        S3ErrorCode::AccessDenied => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Access Denied"})),
        )
            .into_response(),
        _ => (
            err.code.status_code(),
            Json(serde_json::json!({"error": err.message})),
        )
            .into_response(),
    }
}

pub(crate) fn object_not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "Object not found"})),
    )
        .into_response()
}

pub(crate) fn version_not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "Version not found"})),
    )
        .into_response()
}
