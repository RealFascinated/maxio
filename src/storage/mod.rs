pub mod blob;
pub mod cache;
pub mod checksum;
pub mod disk_cache_state;
pub mod hashing;
pub mod lifecycle;
pub mod metadata;
pub mod object_storage;
pub mod orphans;
pub mod pg_metadata;
pub mod traits;

pub use checksum::ChecksumAlgorithm;
pub use lifecycle::{LifecycleAction, LifecycleRule};
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

pub struct PutResult {
    pub size: u64,
    pub etag: String,
    pub last_modified: String,
    pub version_id: Option<String>,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub checksum_value: Option<String>,
}

pub struct DeleteResult {
    pub version_id: Option<String>,
    pub is_delete_marker: bool,
}

/// One entry in a DeleteObjects batch request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchDeleteObject {
    pub key: String,
    pub version_id: Option<String>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersioningState {
    Unversioned,
    Enabled,
    Suspended,
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

impl BucketMeta {
    pub fn new_for_owner(name: String, owner_id: String, owner_display_name: String) -> Self {
        Self {
            name,
            created_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: owner_id.clone(),
            owner_display_name: owner_display_name.clone(),
            acl: Some(crate::iam::Acl::private(&owner_id, &owner_display_name)),
            policy: None,
        }
    }
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

/// Normalize object metadata defaults for owner fields.
/// Object ACL is implicit-private when absent; do not materialize `object_acl_grants` rows here.
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchDeleteOutcome {
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
}

/// Delete object keys in chunks via `delete_objects_batch`.
pub async fn batch_delete_keys(
    storage: &dyn Storage,
    bucket: &str,
    keys: &[String],
    chunk_size: usize,
) -> Result<BatchDeleteOutcome, StorageError> {
    let mut outcome = BatchDeleteOutcome::default();
    if keys.is_empty() {
        return Ok(outcome);
    }
    for chunk in keys.chunks(chunk_size.max(1)) {
        let batch: Vec<BatchDeleteObject> = chunk
            .iter()
            .map(|key| BatchDeleteObject {
                key: key.clone(),
                version_id: None,
            })
            .collect();
        let results = storage.delete_objects_batch(bucket, &batch).await?;
        for (obj, result) in results {
            match result {
                Ok(_) => outcome.succeeded.push(obj.key),
                Err(_) => outcome.failed.push(obj.key),
            }
        }
    }
    Ok(outcome)
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
            .list_objects_page(bucket, prefix, start_after.as_deref(), 1000, None)
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
        let meta = BucketMeta::new_for_owner(
            bucket_name.to_string(),
            default_owner_id(),
            default_owner_display_name(),
        );
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
