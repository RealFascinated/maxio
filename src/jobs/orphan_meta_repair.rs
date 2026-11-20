use std::sync::Arc;

use async_trait::async_trait;

use crate::db::DbPool;
use crate::storage::blob::BlobStorage;
use crate::storage::orphans;
use crate::storage::{MetadataStore, StorageError};

use super::{Job, JobContext, JobHandler, JobOutcome};

/// Deletes metadata rows whose bytes are missing from the data directory and cache.
pub struct OrphanMetaRepairJob {
    pub pool: Arc<DbPool>,
    pub meta: Arc<dyn MetadataStore>,
    pub data_dir: String,
    pub cache_dir: Option<String>,
}

#[async_trait]
impl JobHandler for OrphanMetaRepairJob {
    async fn run(&self, job: &mut Job, _ctx: &JobContext) -> Result<JobOutcome, StorageError> {
        let blobs = BlobStorage::new(&self.data_dir)
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
        let found =
            orphans::scan_orphaned_meta(Arc::clone(&self.pool), &blobs, self.cache_dir.as_deref())
                .await?;
        job.total = Some(found.len() as i64);
        let removed = orphans::delete_orphaned_meta(self.meta.as_ref(), &found).await?;
        job.processed = removed as i64;
        job.detail = serde_json::json!({"removed": removed});
        Ok(JobOutcome::Completed)
    }
}
