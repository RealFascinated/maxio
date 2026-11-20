use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::storage::{MetadataStore, StorageError};

use super::{Job, JobContext, JobHandler, JobKind, JobOutcome, JobStatus, JobTrigger};

/// Finished runs kept per kind and scope.
const KEEP_FINISHED_RUNS: i64 = 20;

type RunKey = (JobKind, Option<String>);

/// Starts, tracks, cancels, resumes and schedules jobs. At most one job runs per kind and scope.
pub struct JobManager {
    meta: Arc<dyn MetadataStore>,
    handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
    running: Mutex<HashMap<RunKey, (Uuid, CancellationToken)>>,
}

impl JobManager {
    pub fn new(
        meta: Arc<dyn MetadataStore>,
        handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            meta,
            handlers,
            running: Mutex::new(HashMap::new()),
        })
    }

    pub async fn list(
        &self,
        kind: Option<JobKind>,
        scope: Option<&str>,
        status: Option<JobStatus>,
        limit: i64,
    ) -> Result<Vec<Job>, StorageError> {
        self.meta.list_jobs(kind, scope, status, limit).await
    }

    pub async fn get(&self, id: Uuid) -> Result<Job, StorageError> {
        self.meta
            .get_job(id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("job {id}")))
    }

    /// Creates and starts a job. `InvalidKey` means the scope is wrong or the job is already running.
    pub async fn start(
        self: &Arc<Self>,
        kind: JobKind,
        scope: Option<String>,
        trigger: JobTrigger,
    ) -> Result<Job, StorageError> {
        if kind.is_scoped() != scope.is_some() {
            return Err(StorageError::InvalidKey(format!(
                "{} jobs {} a bucket scope",
                kind.as_str(),
                if kind.is_scoped() { "need" } else { "take no" }
            )));
        }
        let handler = self.handler(kind)?;
        handler.validate(scope.as_deref()).await?;

        let job = Job::new(kind, scope, trigger);
        let token = self.register(&job)?;
        if let Err(e) = self.meta.save_job(&job).await {
            self.unregister(&job);
            return Err(e);
        }
        tracing::info!(kind = kind.as_str(), scope = ?job.scope, id = %job.id, "job started");
        self.spawn(handler, job.clone(), token);
        Ok(job)
    }

    /// Requests cancellation of a running job. Returns `false` when it is not running here.
    pub fn cancel(&self, id: Uuid) -> bool {
        let running = self.running.lock().unwrap();
        match running.values().find(|(running_id, _)| *running_id == id) {
            Some((_, token)) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Restarts jobs that were still running when the server stopped.
    pub async fn resume(self: &Arc<Self>) -> Result<(), StorageError> {
        for job in self
            .meta
            .list_jobs(None, None, Some(JobStatus::Running), 1000)
            .await?
        {
            let Ok(handler) = self.handler(job.kind) else {
                continue;
            };
            let Ok(token) = self.register(&job) else {
                continue;
            };
            tracing::info!(kind = job.kind.as_str(), scope = ?job.scope, id = %job.id, "resuming job");
            self.spawn(handler, job, token);
        }
        Ok(())
    }

    /// Starts every scheduled kind now and then on its interval.
    pub fn spawn_schedules(self: &Arc<Self>) {
        for kind in JobKind::ALL {
            let Some(interval) = kind.schedule() else {
                continue;
            };
            let manager = Arc::clone(self);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                loop {
                    ticker.tick().await;
                    match manager.start(kind, None, JobTrigger::Schedule).await {
                        Ok(_) | Err(StorageError::InvalidKey(_)) => {}
                        Err(e) => {
                            tracing::warn!(kind = kind.as_str(), "scheduling job failed: {e}")
                        }
                    }
                }
            });
        }
    }

    fn handler(&self, kind: JobKind) -> Result<Arc<dyn JobHandler>, StorageError> {
        self.handlers
            .get(&kind)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(format!("no handler for {}", kind.as_str())))
    }

    fn register(&self, job: &Job) -> Result<CancellationToken, StorageError> {
        let mut running = self.running.lock().unwrap();
        let key = (job.kind, job.scope.clone());
        if running.contains_key(&key) {
            return Err(StorageError::InvalidKey(format!(
                "a {} job is already running{}",
                job.kind.as_str(),
                job.scope
                    .as_ref()
                    .map(|scope| format!(" for {scope}"))
                    .unwrap_or_default()
            )));
        }
        let token = CancellationToken::new();
        running.insert(key, (job.id, token.clone()));
        Ok(token)
    }

    fn unregister(&self, job: &Job) {
        self.running
            .lock()
            .unwrap()
            .remove(&(job.kind, job.scope.clone()));
    }

    fn spawn(
        self: &Arc<Self>,
        handler: Arc<dyn JobHandler>,
        mut job: Job,
        token: CancellationToken,
    ) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let ctx = JobContext {
                token,
                meta: Arc::clone(&manager.meta),
            };
            job.status = match handler.run(&mut job, &ctx).await {
                Ok(JobOutcome::Completed) => JobStatus::Completed,
                Ok(JobOutcome::Cancelled) => JobStatus::Cancelled,
                Err(e) => {
                    tracing::warn!(kind = job.kind.as_str(), scope = ?job.scope, "job failed: {e}");
                    job.error = Some(e.to_string());
                    JobStatus::Failed
                }
            };
            job.finished_at = Some(crate::db::repos::format_ts(chrono::Utc::now()));
            if let Err(e) = manager.meta.save_job(&job).await {
                tracing::warn!(id = %job.id, "saving finished job failed: {e}");
            }
            manager.unregister(&job);
            if let Err(e) = manager
                .meta
                .prune_jobs(job.kind, job.scope.as_deref(), KEEP_FINISHED_RUNS)
                .await
            {
                tracing::warn!("pruning old jobs failed: {e}");
            }
            tracing::info!(
                kind = job.kind.as_str(),
                scope = ?job.scope,
                status = job.status.as_str(),
                processed = job.processed,
                "job finished"
            );
        });
    }
}
