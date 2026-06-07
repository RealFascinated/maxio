use crate::error::S3Error;
use crate::iam::Acl;
use crate::iam::authz::authorize;
use crate::iam::principal::Principal;
use crate::server::AppState;

pub fn get_principal(extensions: &http::Extensions) -> Principal {
    extensions
        .get::<Principal>()
        .cloned()
        .unwrap_or_else(Principal::anonymous)
}

pub struct BucketAuthContext {
    pub policy: Option<String>,
    pub acl: Option<Acl>,
    pub owner_id: String,
    pub owner_display_name: String,
}

pub async fn load_bucket_auth(
    state: &AppState,
    bucket: &str,
) -> Result<BucketAuthContext, S3Error> {
    let snap = state
        .storage
        .fetch_bucket_auth_context(bucket)
        .await
        .map_err(|e| match e {
            crate::storage::StorageError::NotFound(_) => S3Error::no_such_bucket(bucket),
            crate::storage::StorageError::InvalidKey(_) => S3Error::no_such_bucket(bucket),
            e => S3Error::internal(e),
        })?;
    Ok(BucketAuthContext {
        policy: snap.policy,
        acl: snap.acl,
        owner_id: snap.owner_id,
        owner_display_name: snap.owner_display_name,
    })
}

pub async fn check_access(
    state: &AppState,
    principal: &Principal,
    action: &str,
    resource: &str,
    ctx: &BucketAuthContext,
    object_acl: Option<&Acl>,
) -> Result<(), S3Error> {
    // Bucket owner has full control (MinIO-style)
    if !principal.is_root
        && !principal.is_anonymous
        && (principal.canonical_id == ctx.owner_id || principal.user_id == ctx.owner_id)
    {
        return Ok(());
    }
    authorize(
        state,
        principal,
        action,
        resource,
        ctx.policy.as_deref(),
        ctx.acl.as_ref(),
        object_acl,
    )
    .await
}

pub async fn check_bucket_access(
    state: &AppState,
    principal: &Principal,
    bucket: &str,
    action: &str,
) -> Result<BucketAuthContext, S3Error> {
    if principal.is_root {
        return Ok(BucketAuthContext {
            policy: None,
            acl: None,
            owner_id: String::new(),
            owner_display_name: String::new(),
        });
    }

    let ctx = load_bucket_auth(state, bucket).await?;
    let resource = if action == "s3:ListBucket" || action.starts_with("s3:GetBucket") {
        crate::iam::authz::bucket_arn(bucket)
    } else {
        crate::iam::authz::bucket_arn(bucket)
    };
    check_access(state, principal, action, &resource, &ctx, None).await?;
    Ok(ctx)
}

fn is_bucket_owner(principal: &Principal, owner_id: &str) -> bool {
    !principal.is_anonymous && (principal.canonical_id == owner_id || principal.user_id == owner_id)
}

pub async fn check_object_access(
    state: &AppState,
    principal: &Principal,
    bucket: &str,
    key: &str,
    action: &str,
) -> Result<BucketAuthContext, S3Error> {
    if principal.is_root {
        return Ok(BucketAuthContext {
            policy: None,
            acl: None,
            owner_id: String::new(),
            owner_display_name: String::new(),
        });
    }

    let ctx = load_bucket_auth(state, bucket).await?;

    if is_bucket_owner(principal, &ctx.owner_id) {
        return Ok(ctx);
    }

    let object_acl = state.storage.get_object_acl(bucket, key).await.ok();
    check_access(
        state,
        principal,
        action,
        &crate::iam::authz::object_arn(bucket, key),
        &ctx,
        object_acl.as_ref(),
    )
    .await?;
    Ok(ctx)
}
