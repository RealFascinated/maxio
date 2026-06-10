use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::Response,
};

use crate::api::authz::{check_bucket_access, get_principal};
use crate::error::S3Error;
use crate::iam::authz::{authorize, filter_buckets_by_access};
use crate::iam::policy::{parse_policy_json, policy_has_public_list, policy_has_public_read};
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::{BucketMeta, CorsRule, StorageError, is_valid_bucket_name};
use crate::xml::{response::to_xml, types::*};

pub async fn list_buckets(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    let buckets = state
        .storage
        .list_buckets()
        .await
        .map_err(|e| S3Error::internal(e))?;

    let buckets = if principal.is_root {
        buckets
    } else if principal.is_anonymous {
        return Err(S3Error::access_denied("Access Denied"));
    } else {
        authorize(
            &state,
            &principal,
            "s3:ListAllMyBuckets",
            "arn:aws:s3:::*",
            None,
            None,
            None,
        )
        .await?;
        filter_buckets_by_access(&state, &principal, buckets).await
    };

    let owner_id = if principal.is_anonymous {
        crate::iam::ROOT_CANONICAL_ID.to_string()
    } else {
        principal.canonical_id.clone()
    };
    let owner_name = if principal.is_anonymous {
        crate::iam::ROOT_DISPLAY_NAME.to_string()
    } else {
        principal.display_name.clone()
    };

    let result = ListAllMyBucketsResult {
        owner: Owner {
            id: owner_id,
            display_name: owner_name,
        },
        buckets: Buckets {
            bucket: buckets
                .into_iter()
                .map(|b| BucketEntry {
                    name: b.name,
                    creation_date: b.created_at,
                })
                .collect(),
        },
    };

    let xml = to_xml(&result).map_err(|e| S3Error::internal(e))?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub async fn create_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    validate_bucket_name(&bucket)?;
    crate::iam::authz::authorize(
        &state,
        &principal,
        "s3:CreateBucket",
        &crate::iam::authz::bucket_arn(&bucket),
        None,
        None,
        None,
    )
    .await?;

    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

    let meta = BucketMeta {
        name: bucket.clone(),
        created_at: now,
        versioning: false,
        cors_rules: None,
        owner_id: principal.canonical_id.clone(),
        owner_display_name: principal.display_name.clone(),
        acl: Some(crate::iam::Acl::private(
            &principal.canonical_id,
            &principal.display_name,
        )),
        policy: None,
        public_read: false,
        public_list: false,
    };

    let created = state
        .storage
        .create_bucket(&meta)
        .await
        .map_err(|e| S3Error::internal(e))?;

    if !created {
        return Err(S3Error::bucket_already_owned(&bucket));
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Location", format!("/{}", bucket))
        .body(Body::empty())
        .unwrap())
}

pub async fn head_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    check_bucket_access(&state, &principal, &bucket, "s3:ListBucket").await?;
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(StorageError::InvalidKey(_)) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn delete_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    Extension(principal): Extension<Principal>,
) -> Result<Response<Body>, S3Error> {
    if params.contains_key("policy") {
        return delete_bucket_policy(state, bucket, principal).await;
    }
    if params.contains_key("cors") {
        return delete_bucket_cors(state, bucket).await;
    }
    check_bucket_access(&state, &principal, &bucket, "s3:DeleteBucket").await?;
    match state.storage.delete_bucket(&bucket).await {
        Ok(true) => Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap()),
        Ok(false) => Err(S3Error::no_such_bucket(&bucket)),
        Err(StorageError::BucketNotEmpty) => Err(S3Error::bucket_not_empty(&bucket)),
        Err(e) => Err(S3Error::internal(e)),
    }
}

pub async fn handle_bucket_put(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    req: axum::extract::Request,
) -> Result<Response<Body>, S3Error> {
    let principal = get_principal(req.extensions());
    let headers = req.headers().clone();
    let body = req.into_body();
    if params.contains_key("policy") {
        return put_bucket_policy(state, bucket, body, principal).await;
    }
    if params.contains_key("acl") {
        return super::acl::handle_bucket_put_acl(state, bucket, params, headers, body, principal)
            .await;
    }
    if params.contains_key("versioning") {
        return put_bucket_versioning(State(state), Path(bucket), body).await;
    }
    if params.contains_key("cors") {
        return put_bucket_cors(state, bucket, body).await;
    }
    let bucket_name = bucket.clone();
    let state_for_acl = state.clone();
    let resp = create_bucket(State(state), Path(bucket), principal.clone()).await?;
    if resp.status().is_success() {
        let _ = super::acl::apply_create_bucket_acl(
            &state_for_acl,
            &bucket_name,
            &headers,
            &principal.canonical_id,
            &principal.display_name,
        )
        .await;
    }
    Ok(resp)
}

async fn put_bucket_versioning(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .map_err(|e| S3Error::internal(e))?;
    let body_str = String::from_utf8_lossy(&body_bytes);

    // Parse <VersioningConfiguration><Status>Enabled|Suspended</Status></VersioningConfiguration>
    let enabled = if body_str.contains("<Status>Enabled</Status>") {
        true
    } else if body_str.contains("<Status>Suspended</Status>") {
        false
    } else {
        false
    };

    state
        .storage
        .set_versioning(&bucket, enabled)
        .await
        .map_err(|e| S3Error::internal(e))?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_versioning(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    let versioned = state
        .storage
        .is_versioned(&bucket)
        .await
        .map_err(|e| S3Error::internal(e))?;

    let result = VersioningConfiguration {
        status: if versioned {
            Some("Enabled".to_string())
        } else {
            None
        },
    };

    let xml = to_xml(&result).map_err(|e| S3Error::internal(e))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn put_bucket_cors(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(|e| S3Error::internal(e))?;

    let config: CorsConfiguration = quick_xml::de::from_str(&String::from_utf8_lossy(&body_bytes))
        .map_err(|_| S3Error::malformed_xml())?;

    if config.rules.len() > 100 {
        return Err(S3Error::invalid_argument(
            "CORS configuration cannot have more than 100 rules",
        ));
    }
    for rule in &config.rules {
        if rule.allowed_origins.is_empty() || rule.allowed_methods.is_empty() {
            return Err(S3Error::malformed_xml());
        }
        for method in &rule.allowed_methods {
            match method.as_str() {
                "GET" | "PUT" | "POST" | "DELETE" | "HEAD" => {}
                _ => {
                    return Err(S3Error::invalid_argument(&format!(
                        "Invalid HTTP method in CORS rule: {}",
                        method
                    )));
                }
            }
        }
    }

    let rules: Vec<CorsRule> = config
        .rules
        .into_iter()
        .map(|r| CorsRule {
            allowed_origins: r.allowed_origins,
            allowed_methods: r.allowed_methods,
            allowed_headers: r.allowed_headers,
            expose_headers: r.expose_headers,
            max_age_seconds: r.max_age_seconds,
        })
        .collect();

    state
        .storage
        .put_bucket_cors(&bucket, rules)
        .await
        .map_err(|e| S3Error::internal(e))?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_cors(state: AppState, bucket: String) -> Result<Response<Body>, S3Error> {
    let rules = state
        .storage
        .get_bucket_cors(&bucket)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_bucket(&bucket),
            e => S3Error::internal(e),
        })?;

    if rules.is_empty() {
        return Err(S3Error::no_such_cors_configuration());
    }

    let config = CorsConfiguration {
        rules: rules
            .into_iter()
            .map(|r| crate::xml::types::CorsRuleXml {
                allowed_origins: r.allowed_origins,
                allowed_methods: r.allowed_methods,
                allowed_headers: r.allowed_headers,
                expose_headers: r.expose_headers,
                max_age_seconds: r.max_age_seconds,
            })
            .collect(),
    };

    let xml = to_xml(&config).map_err(|e| S3Error::internal(e))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn delete_bucket_cors(state: AppState, bucket: String) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    state
        .storage
        .delete_bucket_cors(&bucket)
        .await
        .map_err(|e| S3Error::internal(e))?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

fn validate_bucket_name(name: &str) -> Result<(), S3Error> {
    if is_valid_bucket_name(name) {
        Ok(())
    } else {
        Err(S3Error::invalid_bucket_name(name))
    }
}

async fn put_bucket_policy(
    state: AppState,
    bucket: String,
    body: Body,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    check_bucket_access(&state, &principal, &bucket, "s3:PutBucketPolicy").await?;
    let body_bytes = axum::body::to_bytes(body, 20 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let policy_str = String::from_utf8_lossy(&body_bytes).trim().to_string();
    parse_policy_json(&policy_str).map_err(|e| S3Error::invalid_argument(&e))?;
    state
        .storage
        .put_bucket_policy(&bucket, &policy_str)
        .await
        .map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_policy(
    state: AppState,
    bucket: String,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    check_bucket_access(&state, &principal, &bucket, "s3:GetBucketPolicy").await?;
    let policy = state
        .storage
        .get_bucket_policy(&bucket)
        .await
        .map_err(S3Error::internal)?;
    let policy =
        policy.ok_or_else(|| S3Error::access_denied("The bucket policy does not exist"))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(policy))
        .unwrap())
}

async fn delete_bucket_policy(
    state: AppState,
    bucket: String,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    check_bucket_access(&state, &principal, &bucket, "s3:DeleteBucketPolicy").await?;
    state
        .storage
        .delete_bucket_policy(&bucket)
        .await
        .map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_policy_status(
    state: AppState,
    bucket: String,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    check_bucket_access(&state, &principal, &bucket, "s3:GetBucketPolicyStatus").await?;
    let policy = state
        .storage
        .get_bucket_policy(&bucket)
        .await
        .map_err(S3Error::internal)?;
    let is_public =
        policy_has_public_read(policy.as_deref()) || policy_has_public_list(policy.as_deref());
    let status = crate::xml::types::PolicyStatus { is_public };
    let xml = crate::xml::response::to_xml(&status).map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}
