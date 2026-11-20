use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Nullable, Text};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use crate::db::DbContext;
use crate::db::schema::jobs;
use crate::jobs::{Job, JobKind, JobStatus, JobTrigger};
use crate::storage::StorageError;

use super::{db_err, format_ts, get_conn, parse_ts};

#[derive(Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = jobs)]
struct JobRow {
    id: Uuid,
    kind: String,
    scope: Option<String>,
    status: String,
    trigger: String,
    processed: i64,
    total: Option<i64>,
    detail: serde_json::Value,
    error: Option<String>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

impl TryFrom<JobRow> for Job {
    type Error = StorageError;

    fn try_from(row: JobRow) -> Result<Self, StorageError> {
        let corrupt =
            |field: &str, value: &str| db_err(format!("job {}: bad {field} {value:?}", row.id));
        Ok(Self {
            id: row.id,
            kind: JobKind::parse(&row.kind).ok_or_else(|| corrupt("kind", &row.kind))?,
            scope: row.scope,
            status: JobStatus::parse(&row.status).ok_or_else(|| corrupt("status", &row.status))?,
            trigger: JobTrigger::parse(&row.trigger)
                .ok_or_else(|| corrupt("trigger", &row.trigger))?,
            processed: row.processed,
            total: row.total,
            detail: row.detail,
            error: row.error,
            started_at: format_ts(row.started_at),
            finished_at: row.finished_at.map(format_ts),
        })
    }
}

fn job_row(job: &Job) -> Result<JobRow, StorageError> {
    Ok(JobRow {
        id: job.id,
        kind: job.kind.as_str().to_string(),
        scope: job.scope.clone(),
        status: job.status.as_str().to_string(),
        trigger: job.trigger.as_str().to_string(),
        processed: job.processed,
        total: job.total,
        detail: job.detail.clone(),
        error: job.error.clone(),
        started_at: parse_ts(&job.started_at)?,
        finished_at: job.finished_at.as_deref().map(parse_ts).transpose()?,
    })
}

pub async fn save_job(ctx: &DbContext, job: &Job) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let row = job_row(job)?;
    diesel::insert_into(jobs::table)
        .values(&row)
        .on_conflict(jobs::id)
        .do_update()
        .set(&row)
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn get_job(ctx: &DbContext, id: Uuid) -> Result<Option<Job>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let row: Option<JobRow> = jobs::table
        .find(id)
        .select(JobRow::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;
    row.map(Job::try_from).transpose()
}

/// Newest first; every filter that is `None` matches all.
pub async fn list_jobs(
    ctx: &DbContext,
    kind: Option<JobKind>,
    scope: Option<&str>,
    status: Option<JobStatus>,
    limit: i64,
) -> Result<Vec<Job>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let mut query = jobs::table.into_boxed();
    if let Some(kind) = kind {
        query = query.filter(jobs::kind.eq(kind.as_str()));
    }
    if let Some(scope) = scope {
        query = query.filter(jobs::scope.eq(scope));
    }
    if let Some(status) = status {
        query = query.filter(jobs::status.eq(status.as_str()));
    }
    let rows: Vec<JobRow> = query
        .order(jobs::started_at.desc())
        .limit(limit)
        .select(JobRow::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;
    rows.into_iter().map(Job::try_from).collect()
}

/// Deletes all but the newest `keep` finished runs of one kind and scope.
pub async fn prune_jobs(
    ctx: &DbContext,
    kind: JobKind,
    scope: Option<&str>,
    keep: i64,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    sql_query(
        "DELETE FROM jobs WHERE kind = $1 AND scope IS NOT DISTINCT FROM $2 AND status <> 'running' \
         AND id NOT IN (SELECT id FROM jobs WHERE kind = $1 AND scope IS NOT DISTINCT FROM $2 \
                        AND status <> 'running' ORDER BY started_at DESC LIMIT $3)",
    )
    .bind::<Text, _>(kind.as_str())
    .bind::<Nullable<Text>, _>(scope)
    .bind::<BigInt, _>(keep)
    .execute(&mut conn)
    .await
    .map_err(db_err)?;
    Ok(())
}
