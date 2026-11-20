use std::sync::Arc;

use async_trait::async_trait;

use crate::storage::{Storage, StorageError};

use super::{Job, JobContext, JobHandler, JobOutcome};

/// Expires objects and versions according to every bucket's lifecycle rules.
pub struct LifecycleJob {
    pub storage: Arc<dyn Storage>,
}

#[async_trait]
impl JobHandler for LifecycleJob {
    fn initial_detail(&self) -> serde_json::Value {
        serde_json::json!({"expired": 0})
    }

    async fn run(&self, job: &mut Job, _ctx: &JobContext) -> Result<JobOutcome, StorageError> {
        let expired = self.storage.lifecycle_sweep().await?;
        job.processed = expired as i64;
        job.detail = serde_json::json!({"expired": expired});
        Ok(JobOutcome::Completed)
    }
}
