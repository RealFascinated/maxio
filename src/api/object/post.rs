use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::HeaderMap,
    response::Response,
};

use crate::api::multipart;
use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::server::AppState;

pub async fn post_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    if params.contains_key("uploads") {
        return multipart::create_multipart_upload(
            State(state),
            Path((bucket, key)),
            Extension(principal),
            headers,
        )
        .await;
    }
    if params.contains_key("uploadId") {
        return multipart::complete_multipart_upload(
            State(state),
            Path((bucket, key)),
            Query(params),
            Extension(principal),
            headers,
            body,
        )
        .await;
    }
    Err(S3Error::not_implemented(
        "Unsupported POST object operation",
    ))
}
