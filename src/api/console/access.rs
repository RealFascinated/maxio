use axum::{Json, http::StatusCode, response::Response};

use crate::api::authz::{check_bucket_access, check_object_access};
use crate::server::AppState;

use super::error::s3_error_to_response;
use super::session::ConsoleSession;

pub(crate) type ConsoleDeny = (StatusCode, Json<serde_json::Value>);

pub(crate) fn console_forbidden() -> ConsoleDeny {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error": "Access Denied"})),
    )
}

pub(crate) async fn console_check(
    state: &AppState,
    session: &ConsoleSession,
    action: &str,
    resource: &str,
    bucket_policy: Option<&str>,
    bucket_acl: Option<&crate::iam::Acl>,
) -> Result<(), ConsoleDeny> {
    crate::iam::authz::authorize(
        state,
        &session.principal(),
        action,
        resource,
        bucket_policy,
        bucket_acl,
        None,
    )
    .await
    .map_err(|_| console_forbidden())
}

pub(crate) async fn console_can(
    state: &AppState,
    session: &ConsoleSession,
    action: &str,
    resource: &str,
) -> bool {
    console_check(state, session, action, resource, None, None)
        .await
        .is_ok()
}

pub(crate) async fn console_bucket_check(
    state: &AppState,
    session: &ConsoleSession,
    bucket: &str,
    action: &str,
) -> Result<(), Response> {
    check_bucket_access(state, &session.principal(), bucket, action)
        .await
        .map(|_| ())
        .map_err(s3_error_to_response)
}

pub(crate) async fn console_bucket_can(
    state: &AppState,
    session: &ConsoleSession,
    bucket: &str,
    action: &str,
) -> bool {
    if session.is_root {
        return true;
    }
    check_bucket_access(state, &session.principal(), bucket, action)
        .await
        .is_ok()
}

pub(crate) async fn console_bucket_can_manage_settings(
    state: &AppState,
    session: &ConsoleSession,
    bucket: &str,
) -> bool {
    for action in [
        "s3:PutBucketVersioning",
        "s3:PutBucketPolicy",
        "s3:PutBucketCors",
        "s3:PutLifecycleConfiguration",
    ] {
        if console_bucket_can(state, session, bucket, action).await {
            return true;
        }
    }
    false
}

pub(crate) async fn console_object_check(
    state: &AppState,
    session: &ConsoleSession,
    bucket: &str,
    key: &str,
    action: &str,
) -> Result<(), Response> {
    check_object_access(state, &session.principal(), bucket, key, action)
        .await
        .map(|_| ())
        .map_err(s3_error_to_response)
}

pub(crate) async fn session_capabilities(
    state: &AppState,
    session: &ConsoleSession,
) -> serde_json::Value {
    serde_json::json!({
        "canCreateBucket": session.is_root
            || console_can(state, session, "s3:CreateBucket", "arn:aws:s3:::*").await,
        "canListAllBuckets": session.is_root
            || console_can(state, session, "s3:ListAllMyBuckets", "arn:aws:s3:::*").await,
        "canManageUsers": session.is_root,
    })
}
