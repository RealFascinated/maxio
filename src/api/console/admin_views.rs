use crate::iam::types::{AccessKey, IamUser, ManagedPolicy};

pub fn user_json(u: &IamUser) -> serde_json::Value {
    serde_json::json!({
        "username": u.username,
        "userId": u.user_id,
        "createdAt": u.created_at,
        "accessKeys": u.access_keys.iter().map(access_key_json).collect::<Vec<_>>(),
        "attachedPolicies": u.attached_policies,
        "inlinePolicies": u.inline_policies.iter().map(|p| p.policy_name.clone()).collect::<Vec<_>>(),
    })
}

pub fn access_key_json(k: &AccessKey) -> serde_json::Value {
    serde_json::json!({
        "accessKeyId": k.access_key_id,
        "status": format!("{:?}", k.status),
        "createdAt": k.created_at,
    })
}

pub fn policy_summary_json(p: &ManagedPolicy) -> serde_json::Value {
    serde_json::json!({
        "name": p.policy_name,
        "policyId": p.policy_id,
        "arn": p.arn,
    })
}

pub fn policy_detail_json(p: &ManagedPolicy) -> serde_json::Value {
    serde_json::json!({
        "name": p.policy_name,
        "arn": p.arn,
        "document": serde_json::to_string(&p.document).unwrap_or_default(),
    })
}

pub fn inline_policy_json(policy_name: &str, document: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "policyName": policy_name,
        "document": serde_json::to_string(document).unwrap_or_default(),
    })
}

pub fn create_user_json(user: &IamUser, key: Option<&AccessKey>) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "username": user.username,
        "userId": user.user_id,
        "accessKey": key.map(|k| serde_json::json!({
            "accessKeyId": k.access_key_id,
            "secretAccessKey": k.secret_access_key,
        })),
    })
}

pub fn create_policy_json(policy: &ManagedPolicy) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "name": policy.policy_name,
        "arn": policy.arn,
    })
}

pub fn create_access_key_json(key: &AccessKey) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "accessKeyId": key.access_key_id,
        "secretAccessKey": key.secret_access_key,
    })
}
