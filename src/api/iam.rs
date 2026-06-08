use std::collections::HashMap;

use axum::{body::Body, extract::State, http::StatusCode, response::Response};

use crate::error::S3Error;
use crate::iam::principal::Principal;
use crate::iam::types::{KeyStatus, PolicyDocumentRaw};
use crate::server::AppState;

pub async fn iam_handler(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> Result<Response<Body>, S3Error> {
    let principal = crate::api::authz::get_principal(req.extensions());
    require_iam_admin(&state, &principal).await?;

    let body = axum::body::to_bytes(req.into_body(), 256 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body)
        .map_err(|_| S3Error::invalid_argument("Invalid form body"))?;
    let action = form
        .get("Action")
        .or_else(|| form.get("action"))
        .cloned()
        .ok_or_else(|| S3Error::invalid_argument("Missing Action"))?;

    let xml = match action.as_str() {
        "CreateUser" => create_user(&state, &form).await?,
        "DeleteUser" => delete_user(&state, &form).await?,
        "GetUser" => get_user(&state, &form).await?,
        "ListUsers" => list_users(&state).await?,
        "CreateAccessKey" => create_access_key(&state, &form).await?,
        "DeleteAccessKey" => delete_access_key(&state, &form).await?,
        "UpdateAccessKey" => update_access_key(&state, &form).await?,
        "ListAccessKeys" => list_access_keys(&state, &form).await?,
        "PutUserPolicy" => put_user_policy(&state, &form).await?,
        "GetUserPolicy" => get_user_policy(&state, &form).await?,
        "DeleteUserPolicy" => delete_user_policy(&state, &form).await?,
        "ListUserPolicies" => list_user_policies(&state, &form).await?,
        "CreatePolicy" => create_policy(&state, &form).await?,
        "DeletePolicy" => delete_policy(&state, &form).await?,
        "GetPolicy" => get_policy(&state, &form).await?,
        "ListPolicies" => list_policies(&state).await?,
        "AttachUserPolicy" => attach_user_policy(&state, &form).await?,
        "DetachUserPolicy" => detach_user_policy(&state, &form).await?,
        "ListAttachedUserPolicies" => list_attached_user_policies(&state, &form).await?,
        other => {
            return Err(S3Error::invalid_argument(&format!(
                "Unknown Action: {other}"
            )));
        }
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn require_iam_admin(state: &AppState, principal: &Principal) -> Result<(), S3Error> {
    if principal.is_root {
        return Ok(());
    }
    if let Some(user) = state.user_store.get_user(&principal.username).await {
        let policies = state.user_store.effective_policies(&user).await;
        for doc in &policies {
            for stmt in &doc.statements {
                if stmt.effect == crate::iam::policy::Effect::Allow
                    && stmt.actions.iter().any(|a| a == "iam:*" || a == "*")
                {
                    return Ok(());
                }
            }
        }
    }
    Err(S3Error::access_denied("Access Denied"))
}

async fn create_user(state: &AppState, form: &HashMap<String, String>) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let user = state
        .user_store
        .create_user(username)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CreateUserResponse><CreateUserResult><User><UserId>{}</UserId><UserName>{}</UserName></User></CreateUserResult></CreateUserResponse>",
        user.user_id, user.username
    ))
}

async fn delete_user(state: &AppState, form: &HashMap<String, String>) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    state
        .user_store
        .delete_user(username)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeleteUserResponse/>".into())
}

async fn get_user(state: &AppState, form: &HashMap<String, String>) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let user = state
        .user_store
        .get_user(username)
        .await
        .ok_or_else(|| S3Error::invalid_argument("NoSuchEntity"))?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><GetUserResponse><GetUserResult><User><UserId>{}</UserId><UserName>{}</UserName><CreateDate>{}</CreateDate></User></GetUserResult></GetUserResponse>",
        user.user_id, user.username, user.created_at
    ))
}

async fn list_users(state: &AppState) -> Result<String, S3Error> {
    let users = state.user_store.list_users().await;
    let mut members = String::new();
    for u in users {
        members.push_str(&format!(
            "<member><UserId>{}</UserId><UserName>{}</UserName><CreateDate>{}</CreateDate></member>",
            u.user_id, u.username, u.created_at
        ));
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListUsersResponse><ListUsersResult><Users>{}</Users></ListUsersResult></ListUsersResponse>",
        members
    ))
}

async fn create_access_key(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let key = state
        .user_store
        .create_access_key(username)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CreateAccessKeyResponse><CreateAccessKeyResult><AccessKey><AccessKeyId>{}</AccessKeyId><SecretAccessKey>{}</SecretAccessKey><Status>Active</Status></AccessKey></CreateAccessKeyResult></CreateAccessKeyResponse>",
        key.access_key_id, key.secret_access_key
    ))
}

async fn delete_access_key(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let access_key_id = form
        .get("AccessKeyId")
        .ok_or_else(|| S3Error::invalid_argument("Missing AccessKeyId"))?;
    state
        .user_store
        .delete_access_key(username, access_key_id)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeleteAccessKeyResponse/>".into())
}

async fn update_access_key(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let access_key_id = form
        .get("AccessKeyId")
        .ok_or_else(|| S3Error::invalid_argument("Missing AccessKeyId"))?;
    let status = form
        .get("Status")
        .map(|s| {
            if s.eq_ignore_ascii_case("Inactive") {
                KeyStatus::Inactive
            } else {
                KeyStatus::Active
            }
        })
        .unwrap_or(KeyStatus::Active);
    state
        .user_store
        .update_access_key_status(username, access_key_id, status)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><UpdateAccessKeyResponse/>".into())
}

async fn list_access_keys(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let user = state
        .user_store
        .get_user(username)
        .await
        .ok_or_else(|| S3Error::invalid_argument("NoSuchEntity"))?;
    let mut members = String::new();
    for k in &user.access_keys {
        let status = match k.status {
            KeyStatus::Active => "Active",
            KeyStatus::Inactive => "Inactive",
        };
        members.push_str(&format!(
            "<member><AccessKeyId>{}</AccessKeyId><Status>{}</Status></member>",
            k.access_key_id, status
        ));
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListAccessKeysResponse><ListAccessKeysResult><AccessKeyMetadata>{}</AccessKeyMetadata></ListAccessKeysResult></ListAccessKeysResponse>",
        members
    ))
}

async fn put_user_policy(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let policy_name = form
        .get("PolicyName")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyName"))?;
    let document = form
        .get("PolicyDocument")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyDocument"))?;
    let doc: PolicyDocumentRaw = serde_json::from_str(document)
        .map_err(|_| S3Error::invalid_argument("MalformedPolicyDocument"))?;
    state
        .user_store
        .put_user_policy(username, policy_name, doc)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><PutUserPolicyResponse/>".into())
}

async fn get_user_policy(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let policy_name = form
        .get("PolicyName")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyName"))?;
    let user = state
        .user_store
        .get_user(username)
        .await
        .ok_or_else(|| S3Error::invalid_argument("NoSuchEntity"))?;
    let policy = user
        .inline_policies
        .iter()
        .find(|p| p.policy_name == *policy_name)
        .ok_or_else(|| S3Error::invalid_argument("NoSuchEntity"))?;
    let doc = serde_json::to_string(&policy.document).map_err(S3Error::internal)?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><GetUserPolicyResponse><GetUserPolicyResult><PolicyDocument>{}</PolicyDocument></GetUserPolicyResult></GetUserPolicyResponse>",
        quick_xml::escape::escape(&doc)
    ))
}

async fn delete_user_policy(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let policy_name = form
        .get("PolicyName")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyName"))?;
    state
        .user_store
        .delete_user_policy(username, policy_name)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeleteUserPolicyResponse/>".into())
}

async fn list_user_policies(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let user = state
        .user_store
        .get_user(username)
        .await
        .ok_or_else(|| S3Error::invalid_argument("NoSuchEntity"))?;
    let mut members = String::new();
    for p in &user.inline_policies {
        members.push_str(&format!("<member>{}</member>", p.policy_name));
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListUserPoliciesResponse><ListUserPoliciesResult><PolicyNames>{}</PolicyNames></ListUserPoliciesResult></ListUserPoliciesResponse>",
        members
    ))
}

async fn create_policy(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let name = form
        .get("PolicyName")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyName"))?;
    let document = form
        .get("PolicyDocument")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyDocument"))?;
    let doc: PolicyDocumentRaw = serde_json::from_str(document)
        .map_err(|_| S3Error::invalid_argument("MalformedPolicyDocument"))?;
    let policy = state
        .user_store
        .create_managed_policy(name, doc)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CreatePolicyResponse><CreatePolicyResult><Policy><PolicyId>{}</PolicyId><PolicyName>{}</PolicyName><Arn>{}</Arn></Policy></CreatePolicyResult></CreatePolicyResponse>",
        policy.policy_id, policy.policy_name, policy.arn
    ))
}

async fn delete_policy(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let name = form
        .get("PolicyName")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyName"))?;
    state
        .user_store
        .delete_managed_policy(name)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeletePolicyResponse/>".into())
}

async fn get_policy(state: &AppState, form: &HashMap<String, String>) -> Result<String, S3Error> {
    let name = form
        .get("PolicyName")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyName"))?;
    let policy = state
        .user_store
        .get_managed_policy(name)
        .await
        .ok_or_else(|| S3Error::invalid_argument("NoSuchEntity"))?;
    let doc = serde_json::to_string(&policy.document).map_err(S3Error::internal)?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><GetPolicyResponse><GetPolicyResult><Policy><PolicyName>{}</PolicyName><Arn>{}</Arn><PolicyDocument>{}</PolicyDocument></Policy></GetPolicyResult></GetPolicyResponse>",
        policy.policy_name,
        policy.arn,
        quick_xml::escape::escape(&doc)
    ))
}

async fn list_policies(state: &AppState) -> Result<String, S3Error> {
    let policies = state.user_store.list_managed_policies().await;
    let mut members = String::new();
    for p in policies {
        members.push_str(&format!(
            "<member><PolicyName>{}</PolicyName><Arn>{}</Arn></member>",
            p.policy_name, p.arn
        ));
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListPoliciesResponse><ListPoliciesResult><Policies>{}</Policies></ListPoliciesResult></ListPoliciesResponse>",
        members
    ))
}

async fn attach_user_policy(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let arn = form
        .get("PolicyArn")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyArn"))?;
    state
        .user_store
        .attach_user_policy(username, arn)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><AttachUserPolicyResponse/>".into())
}

async fn detach_user_policy(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let arn = form
        .get("PolicyArn")
        .ok_or_else(|| S3Error::invalid_argument("Missing PolicyArn"))?;
    state
        .user_store
        .detach_user_policy(username, arn)
        .await
        .map_err(|e| S3Error::invalid_argument(&e))?;
    Ok("<?xml version=\"1.0\" encoding=\"UTF-8\"?><DetachUserPolicyResponse/>".into())
}

async fn list_attached_user_policies(
    state: &AppState,
    form: &HashMap<String, String>,
) -> Result<String, S3Error> {
    let username = form
        .get("UserName")
        .ok_or_else(|| S3Error::invalid_argument("Missing UserName"))?;
    let user = state
        .user_store
        .get_user(username)
        .await
        .ok_or_else(|| S3Error::invalid_argument("NoSuchEntity"))?;
    let mut members = String::new();
    for arn in &user.attached_policies {
        members.push_str(&format!("<member><PolicyArn>{}</PolicyArn></member>", arn));
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListAttachedUserPoliciesResponse><ListAttachedUserPoliciesResult><AttachedPolicies>{}</AttachedPolicies></ListAttachedUserPoliciesResult></ListAttachedUserPoliciesResponse>",
        members
    ))
}
