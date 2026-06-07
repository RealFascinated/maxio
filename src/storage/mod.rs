pub mod blob;
pub mod cache;
pub mod metadata;
pub mod object_storage;
pub mod pg_metadata;
pub mod traits;

pub use metadata::MetadataStore;
pub use object_storage::ObjectStorage;
pub use pg_metadata::PgMetadataStore;
pub use traits::Storage;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use tokio::io::AsyncRead;

pub type ByteStream = Pin<Box<dyn AsyncRead + Send>>;

pub fn validate_bucket_name(name: &str) -> Result<(), StorageError> {
    if is_valid_bucket_name(name) {
        Ok(())
    } else {
        Err(StorageError::InvalidKey(format!(
            "invalid bucket name: {name}"
        )))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    CRC32,
    CRC32C,
    SHA1,
    SHA256,
}

impl ChecksumAlgorithm {
    pub fn header_name(&self) -> &'static str {
        match self {
            Self::CRC32 => "x-amz-checksum-crc32",
            Self::CRC32C => "x-amz-checksum-crc32c",
            Self::SHA1 => "x-amz-checksum-sha1",
            Self::SHA256 => "x-amz-checksum-sha256",
        }
    }

    pub fn from_header_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "CRC32" => Some(Self::CRC32),
            "CRC32C" => Some(Self::CRC32C),
            "SHA1" => Some(Self::SHA1),
            "SHA256" => Some(Self::SHA256),
            _ => None,
        }
    }
}

pub struct PutResult {
    pub size: u64,
    pub etag: String,
    pub version_id: Option<String>,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub checksum_value: Option<String>,
}

pub struct DeleteResult {
    pub version_id: Option<String>,
    pub is_delete_marker: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsRule {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_headers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expose_headers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketMeta {
    pub name: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub versioning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cors_rules: Option<Vec<CorsRule>>,
    #[serde(default = "default_owner_id", skip_serializing_if = "is_root_owner")]
    pub owner_id: String,
    #[serde(
        default = "default_owner_display_name",
        skip_serializing_if = "is_root_owner_display"
    )]
    pub owner_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acl: Option<crate::iam::Acl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    /// Legacy field — migrated to bucket policy on load.
    #[serde(default, skip_serializing_if = "is_false")]
    pub public_read: bool,
    /// Legacy field — migrated to bucket policy on load.
    #[serde(default, skip_serializing_if = "is_false")]
    pub public_list: bool,
}

fn default_owner_id() -> String {
    crate::iam::ROOT_CANONICAL_ID.to_string()
}

fn default_owner_display_name() -> String {
    crate::iam::ROOT_DISPLAY_NAME.to_string()
}

fn is_root_owner(id: &str) -> bool {
    id == crate::iam::ROOT_CANONICAL_ID
}

fn is_root_owner_display(name: &str) -> bool {
    name == crate::iam::ROOT_DISPLAY_NAME
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub content_type: String,
    pub last_modified: String,
    #[serde(default = "default_owner_id", skip_serializing_if = "is_root_owner")]
    pub owner_id: String,
    #[serde(
        default = "default_owner_display_name",
        skip_serializing_if = "is_root_owner_display"
    )]
    pub owner_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acl: Option<crate::iam::Acl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_delete_marker: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_sizes: Option<Vec<u64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartUploadMeta {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    pub content_type: String,
    pub initiated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartMeta {
    pub part_number: u32,
    pub etag: String,
    /// Plaintext byte length of the part (what the client uploaded).
    pub size: u64,
    pub last_modified: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_value: Option<String>,
}

/// Returns `true` if `name` is a valid S3 bucket name.
pub fn is_valid_bucket_name(name: &str) -> bool {
    if name.len() < 3 || name.len() > 63 {
        return false;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return false;
    }

    let first = name.chars().next().unwrap();
    let last = name.chars().last().unwrap();
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return false;
    }

    if name.contains("..") || name.contains(".-") || name.contains("-.") {
        return false;
    }

    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return false;
    }

    true
}

/// Normalize object metadata defaults for owner/acl fields.
pub fn normalize_object_meta(
    meta: &mut ObjectMeta,
    bucket_owner_id: &str,
    bucket_owner_name: &str,
) {
    if meta.owner_id.is_empty() {
        meta.owner_id = bucket_owner_id.to_string();
    }
    if meta.owner_display_name.is_empty() {
        meta.owner_display_name = bucket_owner_name.to_string();
    }
    if meta.acl.is_none() {
        meta.acl = Some(crate::iam::Acl::private(
            &meta.owner_id,
            &meta.owner_display_name,
        ));
    }
}

/// List all objects under `prefix` by paging through `list_objects_page`.
pub async fn list_objects_all(
    storage: &dyn Storage,
    bucket: &str,
    prefix: &str,
) -> Result<Vec<ObjectMeta>, StorageError> {
    let mut all = Vec::new();
    let mut start_after = None;
    loop {
        let page = storage
            .list_objects_page(bucket, prefix, start_after.as_deref(), 1000)
            .await?;
        all.extend(page.objects);
        if !page.is_truncated {
            break;
        }
        start_after = page.next_continuation;
    }
    all.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(all)
}

/// Create each bucket in `default_buckets` (comma-separated) if it does not
/// already exist. Invalid S3 names are logged and skipped; errors are non-fatal.
pub async fn provision_default_buckets(storage: &dyn Storage, default_buckets: &str) {
    if default_buckets.is_empty() {
        return;
    }
    for bucket_name in default_buckets.split(',') {
        let bucket_name = bucket_name.trim();
        if bucket_name.is_empty() {
            continue;
        }
        if !is_valid_bucket_name(bucket_name) {
            tracing::warn!("Skipping invalid default bucket name: '{}'", bucket_name);
            continue;
        }
        let meta = BucketMeta {
            name: bucket_name.to_string(),
            created_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: default_owner_id(),
            owner_display_name: default_owner_display_name(),
            acl: Some(crate::iam::Acl::private(
                &default_owner_id(),
                &default_owner_display_name(),
            )),
            policy: None,
            public_read: false,
            public_list: false,
        };
        match storage.create_bucket(&meta).await {
            Ok(true) => tracing::info!("Created default bucket: {}", bucket_name),
            Ok(false) => tracing::info!("Default bucket already exists: {}", bucket_name),
            Err(e) => tracing::warn!("Failed to create default bucket '{}': {}", bucket_name, e),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Bucket not empty")]
    BucketNotEmpty,
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("Multipart upload not found: {0}")]
    UploadNotFound(String),
    #[error("Version not found: {0}")]
    VersionNotFound(String),
    #[error("Checksum mismatch: {0}")]
    ChecksumMismatch(String),
}

#[cfg(test)]
mod validation_tests {
    use super::validate_bucket_name;

    #[test]
    fn rejects_path_like_bucket_names() {
        for name in [
            "../evil",
            "a/b",
            "ab",
            "evil..bucket",
            "Uppercase",
            "a.-b",
            "a-.b",
            "192.168.0.1",
        ] {
            assert!(
                validate_bucket_name(name).is_err(),
                "{name} should be invalid"
            );
        }
    }

    #[test]
    fn accepts_s3_style_bucket_name() {
        assert!(validate_bucket_name("prod-logs.2026").is_ok());
    }
}
