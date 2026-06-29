use crate::iam::types::{IamUser, KeyStatus, ManagedPolicy};

pub fn create_user_response(user: &IamUser) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CreateUserResponse><CreateUserResult><User><UserId>{}</UserId><UserName>{}</UserName></User></CreateUserResult></CreateUserResponse>",
        user.user_id, user.username
    )
}

pub fn delete_user_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeleteUserResponse/>".into()
}

pub fn get_user_response(user: &IamUser) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><GetUserResponse><GetUserResult><User><UserId>{}</UserId><UserName>{}</UserName><CreateDate>{}</CreateDate></User></GetUserResult></GetUserResponse>",
        user.user_id, user.username, user.created_at
    )
}

pub fn list_users_response(users: &[IamUser]) -> String {
    let mut members = String::new();
    for u in users {
        members.push_str(&format!(
            "<member><UserId>{}</UserId><UserName>{}</UserName><CreateDate>{}</CreateDate></member>",
            u.user_id, u.username, u.created_at
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListUsersResponse><ListUsersResult><Users>{}</Users></ListUsersResult></ListUsersResponse>",
        members
    )
}

pub fn create_access_key_response(access_key_id: &str, secret_access_key: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CreateAccessKeyResponse><CreateAccessKeyResult><AccessKey><AccessKeyId>{}</AccessKeyId><SecretAccessKey>{}</SecretAccessKey><Status>Active</Status></AccessKey></CreateAccessKeyResult></CreateAccessKeyResponse>",
        access_key_id, secret_access_key
    )
}

pub fn delete_access_key_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeleteAccessKeyResponse/>".into()
}

pub fn update_access_key_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><UpdateAccessKeyResponse/>".into()
}

pub fn list_access_keys_response(user: &IamUser) -> String {
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
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListAccessKeysResponse><ListAccessKeysResult><AccessKeyMetadata>{}</AccessKeyMetadata></ListAccessKeysResult></ListAccessKeysResponse>",
        members
    )
}

pub fn put_user_policy_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><PutUserPolicyResponse/>".into()
}

pub fn get_user_policy_response(document: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><GetUserPolicyResponse><GetUserPolicyResult><PolicyDocument>{}</PolicyDocument></GetUserPolicyResult></GetUserPolicyResponse>",
        quick_xml::escape::escape(document)
    )
}

pub fn delete_user_policy_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeleteUserPolicyResponse/>".into()
}

pub fn list_user_policies_response(user: &IamUser) -> String {
    let mut members = String::new();
    for p in &user.inline_policies {
        members.push_str(&format!("<member>{}</member>", p.policy_name));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListUserPoliciesResponse><ListUserPoliciesResult><PolicyNames>{}</PolicyNames></ListUserPoliciesResult></ListUserPoliciesResponse>",
        members
    )
}

pub fn create_policy_response(policy: &ManagedPolicy) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CreatePolicyResponse><CreatePolicyResult><Policy><PolicyId>{}</PolicyId><PolicyName>{}</PolicyName><Arn>{}</Arn></Policy></CreatePolicyResult></CreatePolicyResponse>",
        policy.policy_id, policy.policy_name, policy.arn
    )
}

pub fn delete_policy_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeletePolicyResponse/>".into()
}

pub fn get_policy_response(policy: &ManagedPolicy, document: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><GetPolicyResponse><GetPolicyResult><Policy><PolicyName>{}</PolicyName><Arn>{}</Arn><PolicyDocument>{}</PolicyDocument></Policy></GetPolicyResult></GetPolicyResponse>",
        policy.policy_name,
        policy.arn,
        quick_xml::escape::escape(document)
    )
}

pub fn list_policies_response(policies: &[ManagedPolicy]) -> String {
    let mut members = String::new();
    for p in policies {
        members.push_str(&format!(
            "<member><PolicyName>{}</PolicyName><Arn>{}</Arn></member>",
            p.policy_name, p.arn
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListPoliciesResponse><ListPoliciesResult><Policies>{}</Policies></ListPoliciesResult></ListPoliciesResponse>",
        members
    )
}

pub fn attach_user_policy_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><AttachUserPolicyResponse/>".into()
}

pub fn detach_user_policy_response() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><DetachUserPolicyResponse/>".into()
}

pub fn list_attached_user_policies_response(user: &IamUser) -> String {
    let mut members = String::new();
    for arn in &user.attached_policies {
        members.push_str(&format!("<member><PolicyArn>{}</PolicyArn></member>", arn));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListAttachedUserPoliciesResponse><ListAttachedUserPoliciesResult><AttachedPolicies>{}</AttachedPolicies></ListAttachedUserPoliciesResult></ListAttachedUserPoliciesResponse>",
        members
    )
}
