use crate::error::S3Error;
use crate::iam::acl::{acl_allows, action_to_acl_permission};
use crate::iam::policy::{evaluate, parse_policy_json, AuthDecision};
use crate::iam::principal::Principal;
use crate::iam::Acl;
use crate::server::AppState;

pub fn bucket_arn(bucket: &str) -> String {
    format!("arn:aws:s3:::{bucket}")
}

pub fn object_arn(bucket: &str, key: &str) -> String {
    format!("arn:aws:s3:::{bucket}/{key}")
}

/// Authorize an authenticated principal for an S3 action.
pub fn authorize(
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

    let identity_policies = if let Some(user) = state.user_store.get_user(&principal.username) {
        state.user_store.effective_policies(&user)
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

    match evaluate(
        &principal,
        action,
        resource,
        &[],
        bucket_policy.as_ref(),
    ) {
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
    anonymous_allowed(
        "s3:ListBucket",
        &bucket_arn(bucket),
        policy_json,
        None,
    )
}

pub fn bucket_policy_allows_anonymous_read(
    policy_json: Option<&str>,
    bucket: &str,
    key: &str,
) -> bool {
    anonymous_allowed(
        "s3:GetObject",
        &object_arn(bucket, key),
        policy_json,
        None,
    )
}

pub fn filter_buckets_by_access(
    state: &AppState,
    principal: &Principal,
    buckets: Vec<crate::storage::BucketMeta>,
) -> Vec<crate::storage::BucketMeta> {
    if principal.is_root {
        return buckets;
    }
    buckets
        .into_iter()
        .filter(|b| {
            authorize(
                state,
                principal,
                "s3:ListBucket",
                &bucket_arn(&b.name),
                b.policy.as_deref(),
                b.acl.as_ref(),
                None,
            )
            .is_ok()
                || authorize(
                    state,
                    principal,
                    "s3:GetBucketLocation",
                    &bucket_arn(&b.name),
                    b.policy.as_deref(),
                    b.acl.as_ref(),
                    None,
                )
                .is_ok()
        })
        .collect()
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
