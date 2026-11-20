use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::jobs::{JobKind, JobStatus, JobTrigger};
use crate::server::AppState;
use crate::storage::StorageError;

use super::access::{console_bucket_check, console_forbidden};
use super::session::ConsoleSession;

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 200;

fn job_error_response(e: StorageError) -> Response {
    let status = match e {
        StorageError::NotFound(_) => StatusCode::NOT_FOUND,
        StorageError::InvalidKey(_) => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}

/// Bucket-scoped jobs follow the bucket's settings permissions (compression has no S3 action of
/// its own, so it borrows the versioning ones); every other job is root-only.
async fn authorize(
    state: &AppState,
    session: &ConsoleSession,
    kind: Option<JobKind>,
    scope: Option<&str>,
    write: bool,
) -> Result<(), Response> {
    match (kind, scope) {
        (Some(kind), Some(bucket)) if kind.is_scoped() => {
            let action = if write {
                "s3:PutBucketVersioning"
            } else {
                "s3:GetBucketVersioning"
            };
            console_bucket_check(state, session, bucket, action)
                .await
                .map_err(IntoResponse::into_response)
        }
        _ if session.is_root => Ok(()),
        _ => Err(console_forbidden().into_response()),
    }
}

#[derive(Deserialize)]
pub struct ListJobsQuery {
    kind: Option<JobKind>,
    scope: Option<String>,
    status: Option<JobStatus>,
    limit: Option<i64>,
}

pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Query(query): Query<ListJobsQuery>,
) -> Response {
    if let Err(resp) = authorize(&state, &session, query.kind, query.scope.as_deref(), false).await
    {
        return resp;
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    match state
        .jobs
        .list(query.kind, query.scope.as_deref(), query.status, limit)
        .await
    {
        Ok(jobs) => (StatusCode::OK, Json(serde_json::json!({"jobs": jobs}))).into_response(),
        Err(e) => job_error_response(e),
    }
}

pub async fn get_job(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(id): Path<Uuid>,
) -> Response {
    let job = match state.jobs.get(id).await {
        Ok(job) => job,
        Err(e) => return job_error_response(e),
    };
    if let Err(resp) = authorize(
        &state,
        &session,
        Some(job.kind),
        job.scope.as_deref(),
        false,
    )
    .await
    {
        return resp;
    }
    (StatusCode::OK, Json(serde_json::json!({"job": job}))).into_response()
}

#[derive(Deserialize)]
pub struct StartJobRequest {
    kind: JobKind,
    scope: Option<String>,
}

pub async fn start_job(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Json(body): Json<StartJobRequest>,
) -> Response {
    if let Err(resp) = authorize(
        &state,
        &session,
        Some(body.kind),
        body.scope.as_deref(),
        true,
    )
    .await
    {
        return resp;
    }
    match state
        .jobs
        .start(body.kind, body.scope, JobTrigger::Manual)
        .await
    {
        Ok(job) => {
            tracing::info!(kind = body.kind.as_str(), id = %job.id, "job started via console");
            (StatusCode::OK, Json(serde_json::json!({"job": job}))).into_response()
        }
        Err(e) => job_error_response(e),
    }
}

pub async fn cancel_job(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(id): Path<Uuid>,
) -> Response {
    let job = match state.jobs.get(id).await {
        Ok(job) => job,
        Err(e) => return job_error_response(e),
    };
    if let Err(resp) = authorize(&state, &session, Some(job.kind), job.scope.as_deref(), true).await
    {
        return resp;
    }
    if state.jobs.cancel(id) {
        (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
    } else {
        job_error_response(StorageError::NotFound("job is not running".into()))
    }
}
