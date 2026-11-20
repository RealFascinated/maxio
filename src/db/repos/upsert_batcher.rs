use std::collections::HashMap;
use std::sync::Arc;

use crate::db::DbPool;
use crate::db::schema::objects;
use crate::storage::{ObjectMeta, StorageError};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::RunQueryDsl;
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use super::{db_err, do_upsert_object, get_conn, part_sizes_to_db};

/// Upper bound on rows per statement; keeps bind parameters far below Postgres' 65535 limit.
const MAX_BATCH: usize = 128;

/// Concurrent flushers. Batches stay parallel across connections while each still gets one commit.
const WORKERS: usize = 8;

struct PendingUpsert {
    bucket_id: Uuid,
    meta: ObjectMeta,
    last_modified: chrono::DateTime<chrono::Utc>,
    reply: oneshot::Sender<Result<(), StorageError>>,
}

/// Group commit for plain object upserts.
///
/// Every PUT used to be its own transaction, so concurrent writers serialized on one WAL flush
/// each. Concurrent upserts are now coalesced into a single multi-row `INSERT .. ON CONFLICT`
/// (one round trip, one commit). Callers still wait for the commit, so durability is unchanged.
/// Deletes are deliberately not batched: mixing them in needs an explicit transaction, which
/// costs more round trips than the shared commit saves.
pub struct UpsertBatcher {
    queue: mpsc::UnboundedSender<PendingUpsert>,
}

impl UpsertBatcher {
    pub fn spawn(pool: Arc<DbPool>) -> Self {
        let (queue, receiver) = mpsc::unbounded_channel();
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..WORKERS {
            tokio::spawn(run_worker(Arc::clone(&pool), Arc::clone(&receiver)));
        }
        Self { queue }
    }

    pub async fn upsert(
        &self,
        bucket_id: Uuid,
        meta: &ObjectMeta,
        last_modified: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StorageError> {
        let (reply, response) = oneshot::channel();
        self.queue
            .send(PendingUpsert {
                bucket_id,
                meta: meta.clone(),
                last_modified,
                reply,
            })
            .map_err(|_| db_err("object upsert worker stopped"))?;
        response
            .await
            .map_err(|_| db_err("object upsert worker stopped"))?
    }
}

async fn run_worker(
    pool: Arc<DbPool>,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<PendingUpsert>>>,
) {
    let mut batch = Vec::with_capacity(MAX_BATCH);
    loop {
        // The lock is released before flushing so other workers can collect the next batch.
        if receiver.lock().await.recv_many(&mut batch, MAX_BATCH).await == 0 {
            return;
        }
        flush(&pool, &mut batch).await;
    }
}

async fn flush(pool: &DbPool, batch: &mut Vec<PendingUpsert>) {
    let mut conn = match get_conn(pool).await {
        Ok(conn) => conn,
        Err(e) => {
            let message = e.to_string();
            for pending in batch.drain(..) {
                let _ = pending.reply.send(Err(db_err(&message)));
            }
            return;
        }
    };

    // ON CONFLICT cannot touch the same row twice in one statement: the last write per key wins.
    let latest: HashMap<(Uuid, &str), usize> = batch
        .iter()
        .enumerate()
        .map(|(index, p)| ((p.bucket_id, p.meta.key.as_str()), index))
        .collect();
    let rows: Vec<_> = batch
        .iter()
        .enumerate()
        .filter(|(index, p)| latest[&(p.bucket_id, p.meta.key.as_str())] == *index)
        .map(|(_, p)| {
            (
                objects::id.eq(Uuid::now_v7()),
                objects::bucket_id.eq(p.bucket_id),
                objects::key.eq(&p.meta.key),
                objects::size.eq(p.meta.size as i64),
                objects::etag.eq(&p.meta.etag),
                objects::content_type.eq(&p.meta.content_type),
                objects::last_modified.eq(p.last_modified),
                objects::owner_id.eq(&p.meta.owner_id),
                objects::owner_display_name.eq(&p.meta.owner_display_name),
                objects::version_id.eq(&p.meta.version_id),
                objects::is_delete_marker.eq(p.meta.is_delete_marker),
                objects::is_folder_marker.eq(p.meta.key.ends_with('/')),
                objects::part_sizes.eq(part_sizes_to_db(p.meta.part_sizes.as_deref())),
            )
        })
        .collect();

    let result = diesel::insert_into(objects::table)
        .values(&rows)
        .on_conflict((objects::bucket_id, objects::key))
        .do_update()
        .set((
            objects::size.eq(excluded(objects::size)),
            objects::etag.eq(excluded(objects::etag)),
            objects::content_type.eq(excluded(objects::content_type)),
            objects::last_modified.eq(excluded(objects::last_modified)),
            objects::owner_id.eq(excluded(objects::owner_id)),
            objects::owner_display_name.eq(excluded(objects::owner_display_name)),
            objects::version_id.eq(excluded(objects::version_id)),
            objects::is_delete_marker.eq(excluded(objects::is_delete_marker)),
            objects::is_folder_marker.eq(excluded(objects::is_folder_marker)),
            objects::part_sizes.eq(excluded(objects::part_sizes)),
        ))
        .execute(&mut *conn)
        .await;
    drop(rows);

    match result {
        Ok(_) => {
            for pending in batch.drain(..) {
                let _ = pending.reply.send(Ok(()));
            }
        }
        Err(e) => {
            // One bad row must not fail its neighbours: retry each upsert on its own.
            tracing::warn!("batched object upsert failed, retrying individually: {e}");
            for pending in batch.drain(..) {
                let outcome = do_upsert_object(
                    &mut conn,
                    pending.bucket_id,
                    &pending.meta,
                    pending.last_modified,
                )
                .await;
                let _ = pending.reply.send(outcome);
            }
        }
    }
}
