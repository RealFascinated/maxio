mod buckets;
mod iam;
mod listing;
mod multipart;
mod objects;
mod versions;

pub use buckets::*;
pub use iam::*;
pub use listing::*;
pub use multipart::*;
pub use objects::*;
pub use versions::*;

use crate::db::{BucketCache, CachedBucketEntry, DbPool};
use crate::iam::acl::{AclGrant, AclPermission, Grantee};
use crate::iam::Acl;
use crate::storage::ChecksumAlgorithm;
use crate::storage::StorageError;
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

pub(crate) async fn get_conn(
    pool: &DbPool,
) -> Result<impl std::ops::DerefMut<Target = AsyncPgConnection> + Send, StorageError> {
    pool.get()
        .await
        .map_err(|e| StorageError::Io(std::io::Error::other(e)))
}

pub(crate) fn db_err(e: impl std::fmt::Display) -> StorageError {
    StorageError::Io(std::io::Error::other(e.to_string()))
}

pub(crate) fn format_ts(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub(crate) fn parse_ts(s: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.3fZ")
                .map(|ndt| ndt.and_utc())
        })
        .map_err(|e| db_err(e))
}

pub(crate) fn checksum_to_db(algo: ChecksumAlgorithm) -> &'static str {
    match algo {
        ChecksumAlgorithm::CRC32 => "CRC32",
        ChecksumAlgorithm::CRC32C => "CRC32C",
        ChecksumAlgorithm::SHA1 => "SHA1",
        ChecksumAlgorithm::SHA256 => "SHA256",
    }
}

pub(crate) fn checksum_from_db(s: &str) -> Option<ChecksumAlgorithm> {
    ChecksumAlgorithm::from_header_str(s)
}

pub(crate) fn permission_to_db(p: AclPermission) -> &'static str {
    match p {
        AclPermission::Read => "Read",
        AclPermission::Write => "Write",
        AclPermission::ReadAcp => "ReadAcp",
        AclPermission::WriteAcp => "WriteAcp",
        AclPermission::FullControl => "FullControl",
    }
}

pub(crate) fn permission_from_db(s: &str) -> Result<AclPermission, StorageError> {
    match s {
        "Read" => Ok(AclPermission::Read),
        "Write" => Ok(AclPermission::Write),
        "ReadAcp" => Ok(AclPermission::ReadAcp),
        "WriteAcp" => Ok(AclPermission::WriteAcp),
        "FullControl" => Ok(AclPermission::FullControl),
        other => Err(db_err(format!("unknown ACL permission: {other}"))),
    }
}

pub(crate) fn encode_grantee(grantee: &Grantee) -> (String, Option<String>, Option<String>, Option<String>) {
    match grantee {
        Grantee::CanonicalUser { id, display_name } => (
            "canonical_user".to_string(),
            Some(id.clone()),
            None,
            display_name.clone(),
        ),
        Grantee::Group { uri } => (
            "group".to_string(),
            None,
            Some(uri.clone()),
            None,
        ),
    }
}

pub(crate) fn decode_grantee(
    grantee_type: &str,
    grantee_id: Option<&str>,
    grantee_uri: Option<&str>,
    grantee_display_name: Option<&str>,
) -> Result<Grantee, StorageError> {
    match grantee_type {
        "canonical_user" => Ok(Grantee::CanonicalUser {
            id: grantee_id
                .ok_or_else(|| db_err("canonical_user grant missing id"))?
                .to_string(),
            display_name: grantee_display_name.map(|s| s.to_string()),
        }),
        "group" => Ok(Grantee::Group {
            uri: grantee_uri
                .ok_or_else(|| db_err("group grant missing uri"))?
                .to_string(),
        }),
        other => Err(db_err(format!("unknown grantee type: {other}"))),
    }
}

pub(crate) fn grants_to_acl(
    owner_id: &str,
    owner_display_name: &str,
    rows: &[(String, Option<String>, Option<String>, Option<String>, String)],
) -> Result<Acl, StorageError> {
    let grants = rows
        .iter()
        .map(|(gt, gid, guri, gdn, perm)| {
            Ok(AclGrant {
                grantee: decode_grantee(gt, gid.as_deref(), guri.as_deref(), gdn.as_deref())?,
                permission: permission_from_db(perm)?,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    Ok(Acl {
        owner_id: owner_id.to_string(),
        owner_display_name: owner_display_name.to_string(),
        grants,
    })
}

pub(crate) fn part_sizes_to_db(sizes: Option<&[u64]>) -> Option<Vec<i64>> {
    sizes.map(|s| s.iter().map(|v| *v as i64).collect())
}

pub(crate) fn part_sizes_from_db(sizes: Option<Vec<i64>>) -> Option<Vec<u64>> {
    sizes.map(|s| s.into_iter().map(|v| v as u64).collect())
}

pub(crate) fn escape_like(prefix: &str) -> String {
    prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub(crate) async fn resolve_bucket_id(
    cache: &BucketCache,
    conn: &mut AsyncPgConnection,
    bucket_name: &str,
) -> Result<Uuid, StorageError> {
    if let Some(entry) = cache.get(bucket_name) {
        return Ok(entry.id);
    }

    let entry = buckets::load_bucket_cache_entry(conn, bucket_name).await?;
    cache.insert(bucket_name, entry.clone());
    Ok(entry.id)
}

/// Bucket fields needed before writing object bytes (single round-trip).
#[derive(Debug, Clone)]
pub struct PutBucketContext {
    pub bucket_id: Uuid,
    pub versioning: bool,
    pub owner_id: String,
    pub owner_display_name: String,
}

impl From<CachedBucketEntry> for PutBucketContext {
    fn from(entry: CachedBucketEntry) -> Self {
        Self {
            bucket_id: entry.id,
            versioning: entry.versioning,
            owner_id: entry.owner_id,
            owner_display_name: entry.owner_display_name,
        }
    }
}

/// Policy + ACL fields for authorization (no CORS).
#[derive(Debug, Clone)]
pub struct BucketAuthSnapshot {
    pub policy: Option<String>,
    pub acl: Option<Acl>,
    pub owner_id: String,
    pub owner_display_name: String,
}

impl From<CachedBucketEntry> for BucketAuthSnapshot {
    fn from(entry: CachedBucketEntry) -> Self {
        Self {
            policy: entry.policy,
            acl: entry.acl,
            owner_id: entry.owner_id,
            owner_display_name: entry.owner_display_name,
        }
    }
}
