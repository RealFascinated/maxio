use std::collections::HashMap;

use axum::{
    body::Body,
    http::{HeaderMap, StatusCode},
    response::Response,
};

use crate::api::authz::{check_bucket_access, check_object_access, load_bucket_auth};
use crate::error::S3Error;
use crate::iam::acl::{Acl, CannedAcl};
use crate::iam::principal::Principal;
use crate::server::AppState;
use crate::storage::StorageError;
use crate::xml::{response::to_xml, types::*};

pub async fn handle_bucket_get_acl(
    state: AppState,
    bucket: String,
    params: HashMap<String, String>,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    if !params.contains_key("acl") {
        return Err(S3Error::invalid_argument("Missing acl query parameter"));
    }
    check_bucket_access(&state, &principal, &bucket, "s3:GetBucketAcl").await?;
    let acl = state
        .storage
        .get_bucket_acl(&bucket)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_bucket(&bucket),
            e => S3Error::internal(e),
        })?;
    acl_to_response(acl)
}

pub async fn handle_bucket_put_acl(
    state: AppState,
    bucket: String,
    params: HashMap<String, String>,
    headers: HeaderMap,
    body: Body,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    if !params.contains_key("acl") {
        return Err(S3Error::invalid_argument("Missing acl query parameter"));
    }
    let ctx = check_bucket_access(&state, &principal, &bucket, "s3:PutBucketAcl").await?;
    let acl = parse_acl_input(
        &headers,
        body,
        &principal.canonical_id,
        &principal.display_name,
        Some(&ctx.owner_id),
        Some(&ctx.owner_display_name),
    )
    .await?;
    state
        .storage
        .put_bucket_acl(&bucket, acl)
        .await
        .map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn handle_object_get_acl(
    state: AppState,
    bucket: String,
    key: String,
    params: HashMap<String, String>,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    if !params.contains_key("acl") {
        return Err(S3Error::invalid_argument("Missing acl query parameter"));
    }
    check_object_access(&state, &principal, &bucket, &key, "s3:GetObjectAcl").await?;
    let acl = state
        .storage
        .get_object_acl(&bucket, &key)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(&key),
            e => S3Error::internal(e),
        })?;
    acl_to_response(acl)
}

pub async fn handle_object_put_acl(
    state: AppState,
    bucket: String,
    key: String,
    params: HashMap<String, String>,
    headers: HeaderMap,
    body: Body,
    principal: Principal,
) -> Result<Response<Body>, S3Error> {
    if !params.contains_key("acl") {
        return Err(S3Error::invalid_argument("Missing acl query parameter"));
    }
    let ctx = check_object_access(&state, &principal, &bucket, &key, "s3:PutObjectAcl").await?;
    let acl = parse_acl_input(
        &headers,
        body,
        &principal.canonical_id,
        &principal.display_name,
        Some(&ctx.owner_id),
        Some(&ctx.owner_display_name),
    )
    .await?;
    state
        .storage
        .put_object_acl(&bucket, &key, acl)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_key(&key),
            e => S3Error::internal(e),
        })?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn parse_canned_acl_header(
    headers: &HeaderMap,
    owner_id: &str,
    owner_display_name: &str,
    bucket_owner_id: Option<&str>,
    bucket_owner_display_name: Option<&str>,
) -> Result<Option<Acl>, S3Error> {
    let Some(raw) = headers.get("x-amz-acl").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };
    let canned = CannedAcl::parse(raw)
        .ok_or_else(|| S3Error::invalid_argument(&format!("Invalid canned ACL: {raw}")))?;
    Ok(Some(canned.to_acl(
        owner_id,
        owner_display_name,
        bucket_owner_id,
        bucket_owner_display_name,
    )))
}

async fn parse_acl_input(
    headers: &HeaderMap,
    body: Body,
    owner_id: &str,
    owner_display_name: &str,
    bucket_owner_id: Option<&str>,
    bucket_owner_display_name: Option<&str>,
) -> Result<Acl, S3Error> {
    if let Some(acl) = parse_canned_acl_header(
        headers,
        owner_id,
        owner_display_name,
        bucket_owner_id,
        bucket_owner_display_name,
    )
    .await?
    {
        return Ok(acl);
    }

    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(S3Error::internal)?;
    if body_bytes.is_empty() {
        return Ok(Acl::private(owner_id, owner_display_name));
    }

    let xml_acl: AccessControlPolicy =
        quick_xml::de::from_str(&String::from_utf8_lossy(&body_bytes))
            .map_err(|_| S3Error::malformed_xml())?;
    xml_to_acl(xml_acl)
}

fn xml_to_acl(xml: AccessControlPolicy) -> Result<Acl, S3Error> {
    let owner_id = xml.owner.id.ok_or_else(S3Error::malformed_xml)?;
    let owner_display_name = xml.owner.display_name.unwrap_or_else(|| owner_id.clone());
    let mut grants = Vec::new();
    for g in xml.access_control_list.grants {
        let grantee = if let Some(id) = g.grantee.id {
            crate::iam::Grantee::canonical(&id, g.grantee.display_name.as_deref())
        } else if let Some(uri) = g.grantee.uri {
            crate::iam::Grantee::Group { uri }
        } else {
            continue;
        };
        let permission = match g.permission.as_str() {
            "READ" => crate::iam::AclPermission::Read,
            "WRITE" => crate::iam::AclPermission::Write,
            "READ_ACP" => crate::iam::AclPermission::ReadAcp,
            "WRITE_ACP" => crate::iam::AclPermission::WriteAcp,
            "FULL_CONTROL" => crate::iam::AclPermission::FullControl,
            _ => return Err(S3Error::malformed_xml()),
        };
        grants.push(crate::iam::AclGrant {
            grantee,
            permission,
        });
    }
    Ok(Acl {
        owner_id,
        owner_display_name,
        grants,
    })
}

fn acl_to_response(acl: Acl) -> Result<Response<Body>, S3Error> {
    let xml_acl = AccessControlPolicy {
        owner: OwnerXml {
            id: Some(acl.owner_id),
            display_name: Some(acl.owner_display_name),
        },
        access_control_list: AccessControlList {
            grants: acl
                .grants
                .into_iter()
                .map(|g| GrantXml {
                    grantee: match g.grantee {
                        crate::iam::Grantee::CanonicalUser { id, display_name } => GranteeXml {
                            id: Some(id),
                            display_name,
                            uri: None,
                            xsi_type: Some("CanonicalUser".to_string()),
                        },
                        crate::iam::Grantee::Group { uri } => GranteeXml {
                            id: None,
                            display_name: None,
                            uri: Some(uri),
                            xsi_type: Some("Group".to_string()),
                        },
                    },
                    permission: match g.permission {
                        crate::iam::AclPermission::Read => "READ".to_string(),
                        crate::iam::AclPermission::Write => "WRITE".to_string(),
                        crate::iam::AclPermission::ReadAcp => "READ_ACP".to_string(),
                        crate::iam::AclPermission::WriteAcp => "WRITE_ACP".to_string(),
                        crate::iam::AclPermission::FullControl => "FULL_CONTROL".to_string(),
                    },
                })
                .collect(),
        },
    };
    let xml = to_xml(&xml_acl).map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub async fn apply_create_bucket_acl(
    state: &AppState,
    bucket: &str,
    headers: &HeaderMap,
    owner_id: &str,
    owner_display_name: &str,
) -> Result<(), S3Error> {
    if let Some(acl) =
        parse_canned_acl_header(headers, owner_id, owner_display_name, None, None).await?
    {
        state
            .storage
            .put_bucket_acl(bucket, acl)
            .await
            .map_err(S3Error::internal)?;
    }
    Ok(())
}

pub async fn apply_put_object_acl(
    state: &AppState,
    bucket: &str,
    key: &str,
    headers: &HeaderMap,
    owner_id: &str,
    owner_display_name: &str,
) -> Result<(), S3Error> {
    let ctx = load_bucket_auth(state, bucket).await?;
    if let Some(acl) = parse_canned_acl_header(
        headers,
        owner_id,
        owner_display_name,
        Some(&ctx.owner_id),
        Some(&ctx.owner_display_name),
    )
    .await?
    {
        state
            .storage
            .put_object_acl(bucket, key, acl)
            .await
            .map_err(S3Error::internal)?;
    }
    Ok(())
}
