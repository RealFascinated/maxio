//! Background job that compresses a bucket's existing objects, plus the compression estimator.

use serde::{Deserialize, Serialize};

/// Objects smaller than this are never rewritten (see `compression::MIN_COMPRESS_SIZE`).
pub const MIN_REWRITE_SIZE: i64 = 1024;

/// The job first rewrites current objects, then every stored version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompressionPhase {
    #[default]
    Objects,
    Versions,
}

/// Progress and resume cursor of a bucket's compression job, stored as the job's `detail`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionState {
    pub phase: CompressionPhase,
    /// Last `(key, version_id)` handled in the current phase; resume continues after it.
    pub cursor_key: Option<String>,
    pub cursor_version: Option<String>,
    pub total_bytes: i64,
    pub compressed: i64,
    /// Incompressible, small, or changed by a concurrent write.
    pub skipped: i64,
    pub failed: i64,
    pub bytes_before: i64,
    pub bytes_after: i64,
}

/// An uncompressed object or version still waiting for the job.
#[derive(Debug, Clone)]
pub struct CompressionCandidate {
    pub key: String,
    /// `None` for a current object, `Some` for a stored version.
    pub version_id: Option<String>,
}

/// Result of rewriting one object or version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressOutcome {
    Compressed { before: u64, after: u64 },
    Skipped,
}

/// Projected effect of compressing a bucket's uncompressed objects, from a random sample.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionEstimate {
    pub sampled_objects: usize,
    pub sampled_bytes: u64,
    /// Sampled bytes divided by their compressed size (1.0 = incompressible).
    pub ratio: f64,
    pub candidate_objects: i64,
    pub candidate_bytes: i64,
    pub estimated_saved_bytes: i64,
}

impl CompressionEstimate {
    pub fn extrapolate(
        sampled_objects: usize,
        sampled_bytes: u64,
        sampled_stored: u64,
        candidate_objects: i64,
        candidate_bytes: i64,
    ) -> Self {
        let (ratio, saved_fraction) = if sampled_stored == 0 {
            (1.0, 0.0)
        } else {
            (
                sampled_bytes as f64 / sampled_stored as f64,
                1.0 - sampled_stored as f64 / sampled_bytes as f64,
            )
        };
        Self {
            sampled_objects,
            sampled_bytes,
            ratio,
            candidate_objects,
            candidate_bytes,
            estimated_saved_bytes: (candidate_bytes as f64 * saved_fraction.max(0.0)) as i64,
        }
    }
}

use futures::StreamExt;
use tokio::io::AsyncReadExt;

use super::blob::BlobStorage;
use super::compression;
use super::object_storage::ObjectStorage;
use super::{ObjectMeta, StorageError, validate_bucket_name};

const ESTIMATE_SAMPLE_OBJECTS: i64 = 256;
const ESTIMATE_BYTES_PER_OBJECT: u64 = 1024 * 1024;
const ESTIMATE_MAX_BYTES: u64 = 128 * 1024 * 1024;
const ESTIMATE_CONCURRENCY: usize = 8;

fn is_rewritable(meta: &ObjectMeta) -> bool {
    !meta.is_delete_marker
        && meta.compressed_size.is_none()
        && meta.size >= MIN_REWRITE_SIZE as u64
        && !meta.key.ends_with('/')
        && !compression::is_already_compressed(&meta.key)
}

impl ObjectStorage {
    /// Compresses one current object in place. The key stays locked from the metadata read
    /// to the file swap, so no writer can change the object in between.
    pub(super) async fn compress_object_inner(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<CompressOutcome, StorageError> {
        let _key_guard = self.key_locks.lock(bucket, key).await;
        let meta = match self.meta.get_object_meta(bucket, key).await {
            Ok(meta) => meta,
            Err(StorageError::NotFound(_)) => return Ok(CompressOutcome::Skipped),
            Err(e) => return Err(e),
        };
        if !is_rewritable(&meta) {
            return Ok(CompressOutcome::Skipped);
        }

        let source = self.blobs.open_object(bucket, key, &meta).await?;
        let final_path = self.blobs.object_write_path(bucket, key)?;
        let Some(temp) = self
            .blobs
            .compress_to_temp(&final_path, source, meta.size)
            .await?
        else {
            return Ok(CompressOutcome::Skipped);
        };

        // The size is recorded first: until the swap lands, readers accept either file length.
        let recorded = self
            .meta
            .set_object_compressed_size(bucket, key, &meta.etag, temp.stored_size)
            .await;
        if !matches!(recorded, Ok(true)) {
            let _ = BlobStorage::remove_published_payload(&temp.tmp_path).await;
            return recorded.map(|_| CompressOutcome::Skipped);
        }
        if let Err(e) = BlobStorage::publish_temp_payload(&temp.tmp_path, &final_path).await {
            let _ = BlobStorage::remove_published_payload(&temp.tmp_path).await;
            return Err(e);
        }
        self.blobs
            .complete_object_write(bucket, key, &final_path, temp.stored_size)
            .await?;
        Ok(CompressOutcome::Compressed {
            before: meta.size,
            after: temp.stored_size,
        })
    }

    /// Compresses one stored version in place; see [`Self::compress_object`].
    pub(super) async fn compress_version_inner(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<CompressOutcome, StorageError> {
        let _key_guard = self.key_locks.lock(bucket, key).await;
        let meta = match self
            .meta
            .get_object_version_meta(bucket, key, version_id)
            .await
        {
            Ok(meta) => meta,
            Err(StorageError::NotFound(_) | StorageError::VersionNotFound(_)) => {
                return Ok(CompressOutcome::Skipped);
            }
            Err(e) => return Err(e),
        };
        if !is_rewritable(&meta) {
            return Ok(CompressOutcome::Skipped);
        }

        let source = self
            .blobs
            .open_version(bucket, key, version_id, &meta)
            .await?;
        let final_path = self.blobs.version_data_path(bucket, key, version_id);
        let Some(temp) = self
            .blobs
            .compress_to_temp(&final_path, source, meta.size)
            .await?
        else {
            return Ok(CompressOutcome::Skipped);
        };

        let recorded = self
            .meta
            .set_version_compressed_size(bucket, key, version_id, &meta.etag, temp.stored_size)
            .await;
        if !matches!(recorded, Ok(true)) {
            let _ = BlobStorage::remove_published_payload(&temp.tmp_path).await;
            return recorded.map(|_| CompressOutcome::Skipped);
        }
        if let Err(e) = BlobStorage::publish_temp_payload(&temp.tmp_path, &final_path).await {
            let _ = BlobStorage::remove_published_payload(&temp.tmp_path).await;
            return Err(e);
        }
        Ok(CompressOutcome::Compressed {
            before: meta.size,
            after: temp.stored_size,
        })
    }

    /// Projects the savings of compressing the bucket by compressing a random sample.
    pub(super) async fn estimate_compression_impl(
        &self,
        bucket: &str,
    ) -> Result<CompressionEstimate, StorageError> {
        validate_bucket_name(bucket)?;
        let (candidate_objects, candidate_bytes) =
            self.meta.count_compression_candidates(bucket).await?;
        let sample = self
            .meta
            .sample_compression_candidates(bucket, ESTIMATE_SAMPLE_OBJECTS)
            .await?;

        let measured: Vec<(u64, u64)> = futures::stream::iter(sample)
            .map(|(key, size)| async move {
                let meta = self.meta.get_object_meta(bucket, &key).await.ok()?;
                let length = size.min(ESTIMATE_BYTES_PER_OBJECT);
                let mut stream = self
                    .blobs
                    .open_object_range(bucket, &key, &meta, 0, length)
                    .await
                    .ok()?;
                let mut data = Vec::with_capacity(length as usize);
                stream.read_to_end(&mut data).await.ok()?;
                tokio::task::spawn_blocking(move || {
                    let stored = compression::compress_buffered(&data)
                        .map_or(data.len(), |compressed| compressed.len());
                    (data.len() as u64, stored as u64)
                })
                .await
                .ok()
            })
            .buffer_unordered(ESTIMATE_CONCURRENCY)
            .filter_map(|measurement| async move { measurement })
            .collect()
            .await;

        let (mut sampled_objects, mut sampled_bytes, mut sampled_stored) = (0, 0, 0);
        for (plain, stored) in measured {
            if sampled_bytes >= ESTIMATE_MAX_BYTES {
                break;
            }
            sampled_objects += 1;
            sampled_bytes += plain;
            sampled_stored += stored;
        }
        Ok(CompressionEstimate::extrapolate(
            sampled_objects,
            sampled_bytes,
            sampled_stored,
            candidate_objects,
            candidate_bytes,
        ))
    }
}
