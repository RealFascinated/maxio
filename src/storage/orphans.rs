use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use crate::db::repos::{self, MetaBlobSource};
use crate::db::{DbContext, DbPool};

use super::StorageError;
use super::blob::{BlobStorage, object_path_in};
use super::metadata::MetadataStore;

fn report_progress(label: &str, done: usize, total: usize, final_line: bool) {
    let mut err = io::stderr();
    let _ = write!(err, "\r{label}: {done}/{total}");
    if final_line {
        let _ = writeln!(err);
    } else {
        let _ = err.flush();
    }
}

fn should_report_progress(done: usize, total: usize) -> bool {
    done == 1 || done == total || done % 1000 == 0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanMetaEntry {
    pub bucket: String,
    pub key: String,
    pub source: MetaBlobSource,
}

pub async fn scan_orphaned_meta(
    pool: Arc<DbPool>,
    blobs: &BlobStorage,
    cache_dir: Option<&str>,
) -> Result<Vec<OrphanMetaEntry>, StorageError> {
    let ctx = DbContext::new(pool);
    eprintln!("loading metadata from database...");
    let refs = repos::list_blob_backed_meta(&ctx).await?;
    let total = refs.len();
    eprintln!("checking {total} metadata row(s) on disk...");
    let cache_buckets = cache_dir.map(|dir| Path::new(dir).join("buckets"));

    let mut orphans = Vec::new();
    for (index, reference) in refs.iter().enumerate() {
        let done = index + 1;
        if should_report_progress(done, total) {
            report_progress("scanning", done, total, done == total);
        }
        let missing = match &reference.source {
            MetaBlobSource::Current => {
                !blob_exists(
                    blobs,
                    cache_buckets.as_deref(),
                    &reference.bucket,
                    &reference.key,
                )
                .await
            }
            MetaBlobSource::Version(version_id) => {
                !version_blob_exists(blobs, &reference.bucket, &reference.key, version_id).await
            }
        };
        if missing {
            orphans.push(OrphanMetaEntry {
                bucket: reference.bucket.clone(),
                key: reference.key.clone(),
                source: reference.source.clone(),
            });
        }
    }
    Ok(orphans)
}

pub async fn delete_orphaned_meta(
    meta: &dyn MetadataStore,
    orphans: &[OrphanMetaEntry],
) -> Result<u64, StorageError> {
    let total = orphans.len();
    if total > 0 {
        eprintln!("deleting {total} orphaned metadata row(s)...");
    }
    let mut removed = 0u64;
    for (index, entry) in orphans.iter().enumerate() {
        let done = index + 1;
        if should_report_progress(done, total) {
            report_progress("deleting", done, total, done == total);
        }
        match &entry.source {
            MetaBlobSource::Current => {
                meta.delete_object_meta(&entry.bucket, &entry.key).await?;
                meta.update_current_after_delete(&entry.bucket, &entry.key)
                    .await?;
                removed += 1;
            }
            MetaBlobSource::Version(version_id) => {
                meta.delete_object_version_meta(&entry.bucket, &entry.key, version_id)
                    .await?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

async fn blob_exists(
    blobs: &BlobStorage,
    cache_buckets: Option<&Path>,
    bucket: &str,
    key: &str,
) -> bool {
    if let Some(cache_buckets) = cache_buckets {
        let cache_path = object_path_in(cache_buckets, bucket, key);
        if tokio::fs::try_exists(&cache_path).await.unwrap_or(false) {
            return true;
        }
    }
    tokio::fs::try_exists(blobs.object_path(bucket, key))
        .await
        .unwrap_or(false)
}

async fn version_blob_exists(
    blobs: &BlobStorage,
    bucket: &str,
    key: &str,
    version_id: &str,
) -> bool {
    tokio::fs::try_exists(blobs.version_data_path(bucket, key, version_id))
        .await
        .unwrap_or(false)
}
