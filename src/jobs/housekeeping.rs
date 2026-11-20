use std::sync::Arc;

use async_trait::async_trait;

use crate::storage::{Storage, StorageError};

use super::{Job, JobContext, JobHandler, JobOutcome};

/// Stale multipart uploads are aborted after this long.
const STALE_UPLOAD_AGE_DAYS: i64 = 7;

/// Aborts stale multipart uploads and removes temp files left by crashed writes.
pub struct HousekeepingJob {
    pub storage: Arc<dyn Storage>,
}

#[async_trait]
impl JobHandler for HousekeepingJob {
    async fn run(&self, job: &mut Job, _ctx: &JobContext) -> Result<JobOutcome, StorageError> {
        let (uploads, temp_files) = self
            .storage
            .housekeeping_sweep(chrono::Duration::days(STALE_UPLOAD_AGE_DAYS))
            .await;
        job.processed = (uploads + temp_files) as i64;
        job.detail = serde_json::json!({"uploadsRemoved": uploads, "tempFilesRemoved": temp_files});
        Ok(JobOutcome::Completed)
    }
}
