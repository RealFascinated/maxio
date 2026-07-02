use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};
use tokio::time::{MissedTickBehavior, interval};

use super::DbContext;
use crate::db::repos::flush_deferred_upsert;
use crate::db::repos::PutBucketContext;
use crate::storage::ObjectMeta;

const FLUSH_INTERVAL_MS: u64 = 2;
const FLUSH_BATCH: usize = 128;

pub(crate) struct UpsertJob {
    bucket_name: String,
    meta: ObjectMeta,
    put_ctx: Option<PutBucketContext>,
}

/// Coalescing queue for async metadata upserts (last-write-wins per object key).
#[derive(Clone)]
pub struct AsyncMetaWriter {
    tx: mpsc::UnboundedSender<UpsertJob>,
}

impl AsyncMetaWriter {
    pub(crate) fn from_sender(tx: mpsc::UnboundedSender<UpsertJob>) -> Self {
        Self { tx }
    }

    pub fn enqueue(&self, bucket_name: &str, meta: &ObjectMeta, put_ctx: Option<PutBucketContext>) {
        let _ = self.tx.send(UpsertJob {
            bucket_name: bucket_name.to_string(),
            meta: meta.clone(),
            put_ctx,
        });
    }
}

pub(crate) fn new_channel() -> (
    mpsc::UnboundedSender<UpsertJob>,
    mpsc::UnboundedReceiver<UpsertJob>,
) {
    mpsc::unbounded_channel()
}

pub(crate) fn start_worker(rx: mpsc::UnboundedReceiver<UpsertJob>, ctx: DbContext) {
    let slots = Arc::new(Semaphore::new(32));
    tokio::spawn(run_worker(rx, ctx, slots));
}

async fn run_worker(
    mut rx: mpsc::UnboundedReceiver<UpsertJob>,
    ctx: DbContext,
    slots: Arc<Semaphore>,
) {
    let mut pending: HashMap<(String, String), UpsertJob> = HashMap::new();
    let mut ticker = interval(std::time::Duration::from_millis(FLUSH_INTERVAL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(job) => {
                        let key = (job.bucket_name.clone(), job.meta.key.clone());
                        pending.insert(key, job);
                        if pending.len() >= FLUSH_BATCH {
                            flush_pending(&ctx, &slots, &mut pending).await;
                        }
                    }
                    None => {
                        flush_pending(&ctx, &slots, &mut pending).await;
                        break;
                    }
                }
            }
            _ = ticker.tick() => {
                if !pending.is_empty() {
                    flush_pending(&ctx, &slots, &mut pending).await;
                }
            }
        }
    }
}

async fn flush_pending(
    ctx: &DbContext,
    slots: &Semaphore,
    pending: &mut HashMap<(String, String), UpsertJob>,
) {
    for (_, job) in pending.drain() {
        let _permit = match slots.acquire().await {
            Ok(p) => p,
            Err(_) => return,
        };
        flush_deferred_upsert(ctx, job.bucket_name, job.meta, job.put_ctx).await;
    }
}
