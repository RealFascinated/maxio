use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::server::AppState;

pub async fn list_users_api(State(state): State<AppState>) -> impl IntoResponse {
    let users: Vec<_> = state
        .user_store
        .list_users()
        .await
        .into_iter()
        .map(|u| {
            serde_json::json!({
                "username": u.username,
                "userId": u.user_id,
                "createdAt": u.created_at,
                "accessKeys": u.access_keys.iter().map(|k| serde_json::json!({
                    "accessKeyId": k.access_key_id,
                    "status": format!("{:?}", k.status),
                    "createdAt": k.created_at,
                })).collect::<Vec<_>>(),
                "attachedPolicies": u.attached_policies,
                "inlinePolicies": u.inline_policies.iter().map(|p| p.policy_name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    (StatusCode::OK, Json(serde_json::json!({ "users": users }))).into_response()
}

pub async fn create_user_key_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> impl IntoResponse {
    match state.user_store.create_access_key(&username).await {
        Ok(key) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "accessKeyId": key.access_key_id,
                "secretAccessKey": key.secret_access_key,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn delete_user_key_api(
    State(state): State<AppState>,
    Path((username, access_key_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state
        .user_store
        .delete_access_key(&username, &access_key_id)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct PutUserPolicyRequest {
    document: String,
}

pub async fn put_user_policy_api(
    State(state): State<AppState>,
    Path((username, policy_name)): Path<(String, String)>,
    Json(body): Json<PutUserPolicyRequest>,
) -> impl IntoResponse {
    let doc: crate::iam::types::PolicyDocumentRaw = match serde_json::from_str(&body.document) {
        Ok(d) => d,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid policy document JSON"})),
            )
                .into_response();
        }
    };
    match state
        .user_store
        .put_user_policy(&username, &policy_name, doc)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn get_user_policy_api(
    State(state): State<AppState>,
    Path((username, policy_name)): Path<(String, String)>,
) -> impl IntoResponse {
    let user = match state.user_store.get_user(&username).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "User not found"})),
            )
                .into_response();
        }
    };
    let policy = user
        .inline_policies
        .iter()
        .find(|p| p.policy_name == policy_name);
    match policy {
        Some(p) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "policyName": p.policy_name,
                "document": serde_json::to_string(&p.document).unwrap_or_default(),
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Policy not found"})),
        )
            .into_response(),
    }
}

pub async fn delete_user_policy_api(
    State(state): State<AppState>,
    Path((username, policy_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match state
        .user_store
        .delete_user_policy(&username, &policy_name)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachPolicyRequest {
    policy_arn: String,
}

pub async fn attach_user_policy_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Json(body): Json<AttachPolicyRequest>,
) -> impl IntoResponse {
    match state
        .user_store
        .attach_user_policy(&username, &body.policy_arn)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn detach_user_policy_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Json(body): Json<AttachPolicyRequest>,
) -> impl IntoResponse {
    match state
        .user_store
        .detach_user_policy(&username, &body.policy_arn)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn list_policies_api(State(state): State<AppState>) -> impl IntoResponse {
    let policies: Vec<_> = state
        .user_store
        .list_managed_policies()
        .await
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "name": p.policy_name,
                "policyId": p.policy_id,
                "arn": p.arn,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "policies": policies })),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct CreatePolicyApiRequest {
    name: String,
    document: String,
}

pub async fn create_policy_api(
    State(state): State<AppState>,
    Json(body): Json<CreatePolicyApiRequest>,
) -> impl IntoResponse {
    let doc: crate::iam::types::PolicyDocumentRaw = match serde_json::from_str(&body.document) {
        Ok(d) => d,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid policy document JSON"})),
            )
                .into_response();
        }
    };
    match state
        .user_store
        .create_managed_policy(&body.name, doc)
        .await
    {
        Ok(policy) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "name": policy.policy_name,
                "arn": policy.arn,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn get_policy_api(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.user_store.get_managed_policy(&name).await {
        Some(p) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "name": p.policy_name,
                "arn": p.arn,
                "document": serde_json::to_string(&p.document).unwrap_or_default(),
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Policy not found"})),
        )
            .into_response(),
    }
}

pub async fn delete_policy_api(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.user_store.delete_managed_policy(&name).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateUserApiRequest {
    username: String,
}

pub async fn create_user_api(
    State(state): State<AppState>,
    Json(body): Json<CreateUserApiRequest>,
) -> impl IntoResponse {
    match state.user_store.create_user(&body.username).await {
        Ok(user) => {
            let key = state
                .user_store
                .create_access_key(&user.username)
                .await
                .ok();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "username": user.username,
                    "userId": user.user_id,
                    "accessKey": key.map(|k| serde_json::json!({
                        "accessKeyId": k.access_key_id,
                        "secretAccessKey": k.secret_access_key,
                    })),
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn delete_user_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> impl IntoResponse {
    match state.user_store.delete_user(&username).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}
