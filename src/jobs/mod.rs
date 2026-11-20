//! Generic persisted background jobs: one table, one runner, one handler per [`JobKind`].

mod compress_bucket;
mod housekeeping;
mod lifecycle;
mod manager;
mod orphan_meta_repair;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::storage::{MetadataStore, StorageError};

pub use compress_bucket::CompressBucketJob;
pub use housekeeping::HousekeepingJob;
pub use lifecycle::LifecycleJob;
pub use manager::JobManager;
pub use orphan_meta_repair::OrphanMetaRepairJob;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    /// Rewrites a bucket's existing objects with compression. Scoped to a bucket.
    CompressBucket,
    /// Aborts stale multipart uploads and removes leftover temp files.
    Housekeeping,
    /// Expires objects according to bucket lifecycle rules.
    Lifecycle,
    /// Removes metadata rows whose bytes are missing on disk.
    OrphanMetaRepair,
}

impl JobKind {
    pub const ALL: [JobKind; 4] = [
        Self::CompressBucket,
        Self::Housekeeping,
        Self::Lifecycle,
        Self::OrphanMetaRepair,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompressBucket => "compressBucket",
            Self::Housekeeping => "housekeeping",
            Self::Lifecycle => "lifecycle",
            Self::OrphanMetaRepair => "orphanMetaRepair",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Whether jobs of this kind target one bucket (`scope` = bucket name).
    pub fn is_scoped(self) -> bool {
        matches!(self, Self::CompressBucket)
    }

    /// How often the scheduler starts this kind, if it runs on its own.
    pub fn schedule(self) -> Option<Duration> {
        match self {
            Self::Housekeeping | Self::Lifecycle => Some(Duration::from_secs(3600)),
            Self::CompressBucket | Self::OrphanMetaRepair => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobTrigger {
    Manual,
    Schedule,
}

impl JobTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Schedule => "schedule",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "schedule" => Some(Self::Schedule),
            _ => None,
        }
    }
}

/// A persisted job run. `detail` is owned by the job's handler and doubles as its resume checkpoint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: Uuid,
    pub kind: JobKind,
    /// Bucket name for scoped kinds.
    pub scope: Option<String>,
    pub status: JobStatus,
    pub trigger: JobTrigger,
    pub processed: i64,
    pub total: Option<i64>,
    pub detail: serde_json::Value,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

impl Job {
    pub fn new(kind: JobKind, scope: Option<String>, trigger: JobTrigger) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            scope,
            status: JobStatus::Running,
            trigger,
            processed: 0,
            total: None,
            detail: serde_json::json!({}),
            error: None,
            started_at: crate::db::repos::format_ts(chrono::Utc::now()),
            finished_at: None,
        }
    }

    /// Scope of a scoped job; handlers of scoped kinds are only started with one.
    pub fn bucket(&self) -> Result<&str, StorageError> {
        self.scope
            .as_deref()
            .ok_or_else(|| StorageError::InvalidKey("job has no bucket scope".into()))
    }

    /// Reads the handler's typed state out of `detail`, falling back to its default.
    pub fn state<T: serde::de::DeserializeOwned + Default>(&self) -> T {
        serde_json::from_value(self.detail.clone()).unwrap_or_default()
    }

    pub fn set_state<T: Serialize>(&mut self, state: &T) {
        self.detail = serde_json::to_value(state).unwrap_or_default();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOutcome {
    Completed,
    Cancelled,
}

/// What a running handler can do besides its own work.
pub struct JobContext {
    token: CancellationToken,
    meta: Arc<dyn MetadataStore>,
}

impl JobContext {
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Persists progress so the UI sees it and a restart resumes from it.
    pub async fn checkpoint(&self, job: &Job) -> Result<(), StorageError> {
        self.meta.save_job(job).await
    }
}

/// Behaviour of one [`JobKind`]. Handlers resume from `job.detail`, which they update and
/// [`checkpoint`](JobContext::checkpoint) as they go.
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Rejects a start request before a job is created, e.g. for an unknown bucket.
    async fn validate(&self, _scope: Option<&str>) -> Result<(), StorageError> {
        Ok(())
    }

    /// Runs until done or until `ctx` reports cancellation.
    async fn run(&self, job: &mut Job, ctx: &JobContext) -> Result<JobOutcome, StorageError>;
}
