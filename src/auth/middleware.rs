use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use chrono::{NaiveDateTime, Utc};

use crate::error::S3Error;
use crate::iam::authz::{
    anonymous_allowed, bucket_arn, bucket_policy_allows_anonymous_list,
    bucket_policy_allows_anonymous_read, object_arn,
};
use crate::iam::principal::Principal;
use crate::iam::types::KeyStatus;
use crate::server::AppState;

use super::signature_v4;

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, S3Error> {
    let method = request.method().as_str().to_string();
    let uri = request.uri().to_string();
    let query = request.uri().query().unwrap_or("").to_string();

    tracing::debug!("{} {}", method, uri);

    if query.contains("X-Amz-Signature=") {
        return handle_presigned(&state, &method, &query, request, next).await;
    }

    let has_auth_header = request.headers().get("authorization").is_some();

    if !has_auth_header
        && is_public_bypass_allowed(&state, &method, request.uri().path(), &query).await
    {
        tracing::debug!(
            "Public bucket bypass for {} {}",
            method,
            request.uri().path()
        );
        request.extensions_mut().insert(Principal::anonymous());
        return Ok(next.run(request).await);
    }

    let auth_header = match request.headers().get("authorization") {
        Some(h) => h
            .to_str()
            .map_err(|_| S3Error::access_denied("Invalid Authorization header"))?,
        None => {
            tracing::debug!("No Authorization header present");
            return Err(S3Error::access_denied("Missing Authorization header"));
        }
    };

    let parsed = signature_v4::parse_authorization_header(auth_header)
        .map_err(|e| S3Error::access_denied(e))?;

    if !signature_v4::constant_time_eq(parsed.region.as_bytes(), state.config.region.as_bytes()) {
        return Err(S3Error::access_denied("Invalid region in credential scope"));
    }

    let amz_date = request
        .headers()
        .get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Ok(request_time) = NaiveDateTime::parse_from_str(amz_date, "%Y%m%dT%H%M%SZ") {
        let now = Utc::now().naive_utc();
        let skew = (now - request_time).num_seconds().unsigned_abs();
        if skew > 15 * 60 {
            return Err(S3Error::access_denied(
                "RequestTimeTooSkewed: The difference between the request time and the current time is too large.",
            ));
        }
    } else {
        return Err(S3Error::access_denied(
            "Invalid or missing X-Amz-Date header",
        ));
    }

    let path = request.uri().path().to_string();
    let (secret_key, principal) = resolve_credentials(&state, &parsed.access_key).await?;

    let valid = signature_v4::verify_signature(
        &method,
        &path,
        &query,
        request.headers(),
        &parsed,
        &secret_key,
    );

    if !valid {
        return Err(S3Error::signature_mismatch());
    }

    request.extensions_mut().insert(principal);
    let response = next.run(request).await;
    Ok(response)
}

async fn resolve_credentials(
    state: &AppState,
    access_key: &str,
) -> Result<(String, Principal), S3Error> {
    if signature_v4::constant_time_eq(
        access_key.as_bytes(),
        state.config.access_key.as_bytes(),
    ) {
        return Ok((state.config.secret_key.clone(), Principal::root()));
    }

    if let Some((user, key)) = state.user_store.lookup_by_access_key(access_key).await {
        if key.status == KeyStatus::Active {
            return Ok((key.secret_access_key.clone(), Principal::from_user(&user)));
        }
    }

    Err(S3Error::invalid_access_key())
}

async fn is_public_bypass_allowed(
    state: &AppState,
    method: &str,
    path: &str,
    query: &str,
) -> bool {
    match method {
        "GET" | "HEAD" | "OPTIONS" => {}
        _ => return false,
    }

    for forbidden in [
        "delete",
        "uploads",
        "tagging",
        "versioning",
        "cors",
        "policy",
        "acl",
    ] {
        if has_query_key(query, forbidden) {
            return false;
        }
    }

    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return false;
    }

    let (bucket, rest) = match trimmed.split_once('/') {
        Some((b, r)) => (b, r),
        None => (trimmed, ""),
    };

    if bucket.is_empty() {
        return false;
    }

    let (policy, acl) = match state.storage.get_bucket_auth_info(bucket).await {
        Ok(v) => v,
        Err(_) => return false,
    };

    if rest.is_empty() {
        anonymous_allowed(
            "s3:ListBucket",
            &bucket_arn(bucket),
            policy.as_deref(),
            Some(&acl),
        ) || bucket_policy_allows_anonymous_list(policy.as_deref(), bucket)
    } else {
        anonymous_allowed(
            "s3:GetObject",
            &object_arn(bucket, rest),
            policy.as_deref(),
            Some(&acl),
        ) || bucket_policy_allows_anonymous_read(policy.as_deref(), bucket, rest)
    }
}

fn has_query_key(query: &str, key: &str) -> bool {
    for pair in query.split('&') {
        let name = pair.split('=').next().unwrap_or("");
        if name.eq_ignore_ascii_case(key) {
            return true;
        }
    }
    false
}

async fn handle_presigned(
    state: &AppState,
    method: &str,
    query: &str,
    mut request: Request,
    next: Next,
) -> Result<Response, S3Error> {
    let (parsed, timestamp, expires_secs) =
        signature_v4::parse_presigned_query(query).map_err(|e| S3Error::access_denied(e))?;

    if !signature_v4::constant_time_eq(parsed.region.as_bytes(), state.config.region.as_bytes()) {
        return Err(S3Error::access_denied("Invalid region in credential scope"));
    }

    let issued_at = NaiveDateTime::parse_from_str(&timestamp, "%Y%m%dT%H%M%SZ")
        .map_err(|_| S3Error::access_denied("Invalid X-Amz-Date format"))?;
    let expires_at = issued_at + chrono::Duration::seconds(expires_secs as i64);
    let now = Utc::now().naive_utc();

    if now > expires_at {
        return Err(S3Error::expired_presigned_url());
    }
    if issued_at > now + chrono::Duration::minutes(15) {
        return Err(S3Error::access_denied(
            "X-Amz-Date is too far in the future",
        ));
    }

    let path = request.uri().path().to_string();
    let (secret_key, principal) = resolve_credentials(state, &parsed.access_key).await?;

    let valid = signature_v4::verify_presigned_signature(
        method,
        &path,
        query,
        request.headers(),
        &parsed,
        &timestamp,
        &secret_key,
    );

    if !valid {
        return Err(S3Error::signature_mismatch());
    }

    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}
