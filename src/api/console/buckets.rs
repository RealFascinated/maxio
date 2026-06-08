use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::server::AppState;

use super::access::{
    console_bucket_can, console_bucket_can_manage_settings, console_bucket_check, console_check,
};
use super::session::ConsoleSession;

pub async fn list_buckets(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
) -> impl IntoResponse {
    match state.storage.list_buckets().await {
        Ok(buckets) => {
            let buckets =
                crate::iam::authz::filter_buckets_by_access(&state, &session.principal(), buckets)
                    .await;
            let mut list = Vec::with_capacity(buckets.len());
            for b in buckets {
                let can_delete =
                    console_bucket_can(&state, &session, &b.name, "s3:DeleteBucket").await;
                let can_manage_settings =
                    console_bucket_can_manage_settings(&state, &session, &b.name).await;
                let stat = state.stats.get(&b.name);
                list.push(serde_json::json!({
                    "name": b.name,
                    "createdAt": b.created_at,
                    "versioning": b.versioning,
                    "objectCount": stat.as_ref().map(|s| s.object_count),
                    "sizeBytes": stat.as_ref().map(|s| s.size_bytes),
                    "canDelete": can_delete,
                    "canManageSettings": can_manage_settings,
                }));
            }
            (StatusCode::OK, Json(serde_json::json!({ "buckets": list }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateBucketRequest {
    name: String,
}

pub async fn create_bucket(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Json(body): Json<CreateBucketRequest>,
) -> impl IntoResponse {
    if let Err(deny) = console_check(
        &state,
        &session,
        "s3:CreateBucket",
        &crate::iam::authz::bucket_arn(&body.name),
        None,
        None,
    )
    .await
    {
        return deny.into_response();
    }

    if crate::storage::validate_bucket_name(&body.name).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid bucket name"})),
        )
            .into_response();
    }
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let (owner_id, owner_display_name) = if session.is_root {
        (
            crate::iam::ROOT_CANONICAL_ID.to_string(),
            crate::iam::ROOT_DISPLAY_NAME.to_string(),
        )
    } else {
        (session.user_id.clone(), session.username.clone())
    };
    let meta = crate::storage::BucketMeta {
        name: body.name.clone(),
        created_at: now,
        versioning: false,
        cors_rules: None,
        owner_id: owner_id.clone(),
        owner_display_name: owner_display_name.clone(),
        acl: Some(crate::iam::Acl::private(&owner_id, &owner_display_name)),
        policy: None,
        public_read: false,
        public_list: false,
    };

    match state.storage.create_bucket(&meta).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Bucket already exists"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_bucket_api(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:DeleteBucket").await {
        return resp;
    }

    match state.storage.delete_bucket(&bucket).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bucket not found"})),
        )
            .into_response(),
        Err(crate::storage::StorageError::BucketNotEmpty) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Bucket is not empty"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
