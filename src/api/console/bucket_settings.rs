use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::server::AppState;

use super::access::console_bucket_check;
use super::session::ConsoleSession;

pub async fn get_versioning(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_bucket_check(&state, &session, &bucket, "s3:GetBucketVersioning").await
    {
        return resp;
    }

    match state.storage.is_versioned(&bucket).await {
        Ok(enabled) => (
            StatusCode::OK,
            Json(serde_json::json!({"enabled": enabled})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetVersioningRequest {
    enabled: bool,
}

pub async fn set_versioning(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<SetVersioningRequest>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_bucket_check(&state, &session, &bucket, "s3:PutBucketVersioning").await
    {
        return resp;
    }

    match state.storage.set_versioning(&bucket, body.enabled).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_public(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:GetBucketPolicy").await {
        return resp;
    }

    match state.storage.get_bucket_policy(&bucket).await {
        Ok(policy) => {
            let read = crate::iam::policy::policy_has_public_read(policy.as_deref());
            let list = crate::iam::policy::policy_has_public_list(policy.as_deref());
            (
                StatusCode::OK,
                Json(serde_json::json!({"read": read, "list": list})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetPublicRequest {
    read: bool,
    list: bool,
}

pub async fn get_cors(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:GetBucketCors").await {
        return resp;
    }

    match state.storage.get_bucket_cors(&bucket).await {
        Ok(rules) => {
            let enabled = crate::api::cors::cors_has_console_permissive(&rules);
            (
                StatusCode::OK,
                Json(serde_json::json!({"enabled": enabled})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetCorsRequest {
    enabled: bool,
}

pub async fn set_cors(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<SetCorsRequest>,
) -> impl IntoResponse {
    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:PutBucketCors").await {
        return resp;
    }

    let result = if body.enabled {
        state
            .storage
            .put_bucket_cors(&bucket, crate::api::cors::console_permissive_cors_rules())
            .await
    } else {
        state.storage.delete_bucket_cors(&bucket).await
    };

    match result {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn set_public(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<SetPublicRequest>,
) -> impl IntoResponse {
    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:PutBucketPolicy").await {
        return resp;
    }

    let existing = match state.storage.get_bucket_policy(&bucket).await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    let policy = match crate::iam::policy::merge_public_access_policy(
        &bucket,
        existing.as_deref(),
        body.read,
        body.list,
    ) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e})),
            )
                .into_response();
        }
    };
    match state.storage.put_bucket_policy(&bucket, &policy).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
