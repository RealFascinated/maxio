use crate::error::S3Error;
use crate::iam::Acl;
use crate::iam::acl::{acl_allows, action_to_acl_permission};
use crate::iam::policy::{AuthDecision, PolicyDocument, evaluate, parse_policy_json};
use crate::iam::principal::Principal;
use crate::server::AppState;

pub fn bucket_arn(bucket: &str) -> String {
    format!("arn:aws:s3:::{bucket}")
}

pub fn object_arn(bucket: &str, key: &str) -> String {
    format!("arn:aws:s3:::{bucket}/{key}")
}

/// Authorize an authenticated principal for an S3 action.
pub async fn authorize(
    state: &AppState,
    principal: &Principal,
    action: &str,
    resource: &str,
    bucket_policy_json: Option<&str>,
    bucket_acl: Option<&Acl>,
    object_acl: Option<&Acl>,
) -> Result<(), S3Error> {
    if principal.is_root {
        return Ok(());
    }

    if principal.is_anonymous {
        return authorize_anonymous(action, resource, bucket_policy_json, bucket_acl, object_acl);
    }

    let identity_policies = if let Some(user) = state.user_store.get_user(&principal.username).await
    {
        state.user_store.effective_policies(&user).await
    } else {
        vec![]
    };

    let bucket_policy = bucket_policy_json.and_then(|j| parse_policy_json(j).ok());

    match evaluate(
        principal,
        action,
        resource,
        &identity_policies,
        bucket_policy.as_ref(),
    ) {
        AuthDecision::Allow => return Ok(()),
        AuthDecision::Deny => return Err(S3Error::access_denied("Access Denied")),
        AuthDecision::NoMatch => {}
    }

    // Fall back to ACL checks
    let acl = object_acl.or(bucket_acl);
    if let Some(acl) = acl {
        if let Some(perm) = action_to_acl_permission(action) {
            if acl_allows(acl, principal, perm) {
                return Ok(());
            }
        }
    }

    // Bucket owner gets implicit full control (MinIO-style)
    if action.starts_with("s3:") {
        // Handled via owner checks in handlers when needed
    }

    Err(S3Error::access_denied("Access Denied"))
}

pub fn authorize_anonymous(
    action: &str,
    resource: &str,
    bucket_policy_json: Option<&str>,
    bucket_acl: Option<&Acl>,
    object_acl: Option<&Acl>,
) -> Result<(), S3Error> {
    let principal = Principal::anonymous();
    let bucket_policy = bucket_policy_json.and_then(|j| parse_policy_json(j).ok());

    match evaluate(&principal, action, resource, &[], bucket_policy.as_ref()) {
        AuthDecision::Allow => return Ok(()),
        AuthDecision::Deny => return Err(S3Error::access_denied("Access Denied")),
        AuthDecision::NoMatch => {}
    }

    let acl = object_acl.or(bucket_acl);
    if let Some(acl) = acl {
        if let Some(perm) = action_to_acl_permission(action) {
            if acl_allows(acl, &principal, perm) {
                return Ok(());
            }
        }
    }

    Err(S3Error::access_denied("Access Denied"))
}

/// Check whether anonymous access is allowed for a read operation (auth middleware).
pub fn anonymous_allowed(
    action: &str,
    resource: &str,
    bucket_policy_json: Option<&str>,
    bucket_acl: Option<&Acl>,
) -> bool {
    authorize_anonymous(action, resource, bucket_policy_json, bucket_acl, None).is_ok()
}

pub fn bucket_policy_allows_anonymous_list(policy_json: Option<&str>, bucket: &str) -> bool {
    anonymous_allowed("s3:ListBucket", &bucket_arn(bucket), policy_json, None)
}

pub fn bucket_policy_allows_anonymous_read(
    policy_json: Option<&str>,
    bucket: &str,
    key: &str,
) -> bool {
    anonymous_allowed("s3:GetObject", &object_arn(bucket, key), policy_json, None)
}

pub async fn filter_buckets_by_access(
    state: &AppState,
    principal: &Principal,
    buckets: Vec<crate::storage::BucketMeta>,
) -> Vec<crate::storage::BucketMeta> {
    if principal.is_root {
        return buckets;
    }

    // Load identity policies once for the whole list rather than once per authorize() call.
    let identity_policies: Vec<PolicyDocument> = if principal.is_anonymous {
        vec![]
    } else if let Some(user) = state.user_store.get_user(&principal.username).await {
        state.user_store.effective_policies(&user).await
    } else {
        vec![]
    };

    let mut out = Vec::new();
    for b in buckets {
        let bucket_policy = b.policy.as_deref().and_then(|j| parse_policy_json(j).ok());
        let arn = bucket_arn(&b.name);

        let allowed = bucket_allowed_by_policy_or_acl(
            principal,
            &arn,
            &identity_policies,
            bucket_policy.as_ref(),
            b.acl.as_ref(),
        );
        if allowed {
            out.push(b);
        }
    }
    out
}

/// Returns true if the principal is allowed `s3:ListBucket` or `s3:GetBucketLocation`
/// on a bucket, using pre-loaded identity policies.
fn bucket_allowed_by_policy_or_acl(
    principal: &Principal,
    arn: &str,
    identity_policies: &[PolicyDocument],
    bucket_policy: Option<&crate::iam::policy::PolicyDocument>,
    bucket_acl: Option<&Acl>,
) -> bool {
    for action in ["s3:ListBucket", "s3:GetBucketLocation"] {
        match evaluate(principal, action, arn, identity_policies, bucket_policy) {
            AuthDecision::Allow => return true,
            AuthDecision::Deny => continue,
            AuthDecision::NoMatch => {}
        }
        if let Some(acl) = bucket_acl {
            if let Some(perm) = action_to_acl_permission(action) {
                if acl_allows(acl, principal, perm) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arn_format() {
        assert_eq!(bucket_arn("my-bucket"), "arn:aws:s3:::my-bucket");
        assert_eq!(
            object_arn("my-bucket", "path/file.txt"),
            "arn:aws:s3:::my-bucket/path/file.txt"
        );
    }
}
