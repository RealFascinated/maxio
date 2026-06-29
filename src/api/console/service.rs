use axum::http::HeaderMap;
use hmac::{KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::auth::signature_v4;
use crate::config::Config;
use crate::storage::{
    BatchDeleteOutcome, ObjectMeta, Storage, StorageError, batch_delete_keys, list_objects_all,
};

use super::types::{ObjectDeleteOp, ObjectGetOp, ObjectGetResult, PresignResult};

type HmacSha256 = hmac::Hmac<Sha256>;

pub const CONSOLE_DELETE_BATCH: usize = 1000;

pub struct PresignContext<'a> {
    pub headers: &'a HeaderMap,
    pub config: &'a Config,
    pub access_key: String,
    pub secret_key: String,
}

pub struct ConsoleService<'a> {
    pub storage: &'a dyn Storage,
}

impl ConsoleService<'_> {
    pub async fn ensure_bucket(&self, bucket: &str) -> Result<(), StorageError> {
        match self.storage.head_bucket(bucket).await? {
            true => Ok(()),
            false => Err(StorageError::NotFound(bucket.to_string())),
        }
    }

    pub async fn get_object_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMeta, StorageError> {
        let mut meta = self.storage.head_object(bucket, key).await?;
        if let Ok(tags) = self.storage.get_object_tagging(bucket, key).await {
            if !tags.is_empty() {
                meta.tags = Some(tags);
            }
        }
        Ok(meta)
    }

    pub async fn get_object(
        &self,
        bucket: &str,
        key: &str,
        op: ObjectGetOp,
        presign: Option<PresignContext<'_>>,
    ) -> Result<ObjectGetResult, StorageError> {
        match op {
            ObjectGetOp::Metadata => {
                let meta = self.get_object_metadata(bucket, key).await?;
                Ok(ObjectGetResult::Metadata(meta))
            }
            ObjectGetOp::Download => {
                let (reader, meta) = self.storage.get_object(bucket, key).await?;
                Ok(ObjectGetResult::Attachment { reader, meta })
            }
            ObjectGetOp::DownloadVersion { version_id } => {
                let (reader, meta) = self
                    .storage
                    .get_object_version(bucket, key, &version_id)
                    .await?;
                Ok(ObjectGetResult::Attachment { reader, meta })
            }
            ObjectGetOp::Presign { expires_secs } => {
                self.storage.head_object(bucket, key).await?;
                let ctx = presign.ok_or_else(|| {
                    StorageError::InvalidKey("presign credentials required".into())
                })?;
                let url = build_presigned_url(
                    ctx.headers,
                    ctx.config,
                    &ctx.access_key,
                    &ctx.secret_key,
                    bucket,
                    key,
                    expires_secs,
                );
                Ok(ObjectGetResult::Presign(PresignResult {
                    url,
                    expires_secs,
                }))
            }
        }
    }

    pub async fn delete_object(
        &self,
        bucket: &str,
        key: &str,
        op: ObjectDeleteOp,
    ) -> Result<(), StorageError> {
        match op {
            ObjectDeleteOp::Current => {
                self.storage.delete_object(bucket, key).await?;
            }
            ObjectDeleteOp::Version { version_id } => {
                self.storage
                    .delete_object_version(bucket, key, &version_id)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn batch_delete(
        &self,
        bucket: &str,
        keys: &[String],
    ) -> Result<BatchDeleteOutcome, StorageError> {
        batch_delete_keys(self.storage, bucket, keys, CONSOLE_DELETE_BATCH).await
    }

    pub async fn preserve_parent_folder(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(), StorageError> {
        preserve_empty_parent_folder_after_object_delete(self.storage, bucket, key).await
    }
}

pub fn parent_folder_prefix_for_deleted_object(key: &str) -> Option<String> {
    if key.ends_with('/') {
        return None;
    }
    key.rfind('/')
        .map(|idx| key[..=idx].to_string())
        .filter(|prefix| !prefix.is_empty())
}

pub async fn preserve_empty_parent_folder_after_object_delete(
    storage: &dyn Storage,
    bucket: &str,
    key: &str,
) -> Result<(), StorageError> {
    let Some(parent_prefix) = parent_folder_prefix_for_deleted_object(key) else {
        return Ok(());
    };

    let remaining = list_objects_all(storage, bucket, &parent_prefix).await?;

    let parent_still_exists = remaining
        .iter()
        .any(|obj| obj.key == parent_prefix || obj.key.starts_with(&parent_prefix));
    if parent_still_exists {
        return Ok(());
    }

    storage
        .put_object(
            bucket,
            &parent_prefix,
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
        )
        .await?;
    Ok(())
}

pub async fn folder_delete_stats(
    storage: &dyn Storage,
    bucket: &str,
    prefixes: &[String],
) -> Result<(usize, u64), StorageError> {
    let mut count = 0usize;
    let mut size_bytes = 0u64;
    for prefix in prefixes {
        let objects = list_objects_all(storage, bucket, prefix).await?;
        count += objects.len();
        size_bytes += objects.iter().map(|obj| obj.size).sum::<u64>();
    }
    Ok((count, size_bytes))
}

pub fn normalize_folder_prefix(name: &str) -> Option<String> {
    let trimmed = name.trim().trim_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(format!("{trimmed}/"))
    }
}

pub fn normalize_presign_host(host: &str, scheme: &str) -> String {
    let host = host.split(',').next().unwrap_or(host).trim();
    if scheme == "https" {
        host.trim_end_matches(":443").to_string()
    } else if scheme == "http" {
        host.trim_end_matches(":80").to_string()
    } else {
        host.to_string()
    }
}

fn presign_endpoint(headers: &HeaderMap, config: &Config) -> (String, String) {
    if let Some(base) = config.public_url.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(uri) = base.parse::<http::Uri>() {
            let scheme = uri.scheme_str().unwrap_or("https").to_string();
            if let Some(authority) = uri.authority() {
                return (
                    scheme.clone(),
                    normalize_presign_host(authority.as_str(), &scheme),
                );
            }
        }
    }

    let scheme = presign_scheme(headers).to_string();
    let raw_host = headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("host").and_then(|v| v.to_str().ok()))
        .unwrap_or("localhost:9000");

    let host =
        if config.allow_insecure_dev && matches!(raw_host, "localhost:5173" | "127.0.0.1:5173") {
            format!("127.0.0.1:{}", config.port)
        } else {
            raw_host.to_string()
        };

    (scheme.clone(), normalize_presign_host(&host, &scheme))
}

fn presign_scheme(headers: &HeaderMap) -> &'static str {
    if headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
    {
        "https"
    } else {
        "http"
    }
}

fn build_presigned_url(
    headers: &HeaderMap,
    config: &Config,
    access_key: &str,
    secret_key: &str,
    bucket: &str,
    key: &str,
    expires_secs: u64,
) -> String {
    let (scheme, host) = presign_endpoint(headers, config);

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let region = "us-east-1";

    let credential = format!("{}/{}/{}/s3/aws4_request", access_key, date_stamp, region);

    const S3_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    let encode =
        |s: &str| -> String { percent_encoding::utf8_percent_encode(s, S3_ENCODE).to_string() };

    let encoded_key: String = key
        .split('/')
        .map(|s| encode(s))
        .collect::<Vec<_>>()
        .join("/");
    let path = format!("/{}/{}", encode(bucket), encoded_key);

    let qs_params = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential", credential.clone()),
        ("X-Amz-Date", amz_date.clone()),
        ("X-Amz-Expires", expires_secs.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];

    let canonical_qs: String = qs_params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!("host:{}\n", host);
    let canonical_request = format!(
        "GET\n{}\n{}\n{}\nhost\nUNSIGNED-PAYLOAD",
        path, canonical_qs, canonical_headers
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, scope, canonical_hash
    );

    let signing_key = signature_v4::derive_signing_key(secret_key, &date_stamp, region);

    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    format!(
        "{}://{}{}?{}&X-Amz-Signature={}",
        scheme, host, path, canonical_qs, signature
    )
}
