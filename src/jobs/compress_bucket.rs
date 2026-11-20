use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;

use crate::storage::compression_job::{CompressOutcome, CompressionPhase, CompressionState};
use crate::storage::{MetadataStore, Storage, StorageError, validate_bucket_name};

use super::{Job, JobContext, JobHandler, JobOutcome};

const BATCH: i64 = 200;
const CONCURRENCY: usize = 4;

/// Compresses a bucket's existing objects (then its stored versions) with the bucket's codec.
pub struct CompressBucketJob {
    pub storage: Arc<dyn Storage>,
    pub meta: Arc<dyn MetadataStore>,
}

#[async_trait]
impl JobHandler for CompressBucketJob {
    async fn validate(&self, scope: Option<&str>) -> Result<(), StorageError> {
        let bucket = scope.unwrap_or_default();
        validate_bucket_name(bucket)?;
        self.meta.get_compression(bucket).await.map(|_| ())
    }

    async fn run(&self, job: &mut Job, ctx: &JobContext) -> Result<JobOutcome, StorageError> {
        let bucket = job.bucket()?.to_string();
        let mut state: CompressionState = job.state();
        if job.total.is_none() {
            let (objects, bytes) = self.meta.count_compression_candidates(&bucket).await?;
            job.total = Some(objects);
            state.total_bytes = bytes;
            job.set_state(&state);
            ctx.checkpoint(job).await?;
        }

        loop {
            if ctx.is_cancelled() {
                return Ok(JobOutcome::Cancelled);
            }
            let batch = self
                .meta
                .list_compression_candidates(&bucket, &state, BATCH)
                .await?;
            let Some(last) = batch.last() else {
                if state.phase == CompressionPhase::Versions {
                    return Ok(JobOutcome::Completed);
                }
                state.phase = CompressionPhase::Versions;
                state.cursor_key = None;
                state.cursor_version = None;
                job.set_state(&state);
                ctx.checkpoint(job).await?;
                continue;
            };
            let next_cursor = (last.key.clone(), last.version_id.clone());

            let storage = &self.storage;
            let bucket_ref = bucket.as_str();
            let outcomes: Vec<_> = futures::stream::iter(batch.clone())
                .map(|candidate| async move {
                    match &candidate.version_id {
                        None => storage.compress_object(bucket_ref, &candidate.key).await,
                        Some(version_id) => {
                            storage
                                .compress_version(bucket_ref, &candidate.key, version_id)
                                .await
                        }
                    }
                })
                .buffer_unordered(CONCURRENCY)
                .collect()
                .await;

            job.processed += batch.len() as i64;
            for outcome in outcomes {
                match outcome {
                    Ok(CompressOutcome::Compressed { before, after }) => {
                        state.compressed += 1;
                        state.bytes_before += before as i64;
                        state.bytes_after += after as i64;
                    }
                    Ok(CompressOutcome::Skipped) => state.skipped += 1,
                    Err(e) => {
                        tracing::warn!(bucket = %bucket, "compressing object failed: {e}");
                        state.failed += 1;
                    }
                }
            }
            (state.cursor_key, state.cursor_version) = (Some(next_cursor.0), next_cursor.1);
            job.set_state(&state);
            ctx.checkpoint(job).await?;
        }
    }
}
