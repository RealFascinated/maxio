use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::db::DbContext;
use crate::db::schema::{bucket_lifecycle_rules, buckets, object_versions, objects};
use crate::storage::{LifecycleAction, LifecycleRule, StorageError, validate_bucket_name};

use super::{db_err, escape_like, get_conn, resolve_bucket_id};

#[derive(Debug, Clone)]
pub struct BucketLifecycleEntry {
    pub bucket_name: String,
    pub bucket_id: Uuid,
    pub rules: Vec<LifecycleRule>,
}

#[derive(Debug, Clone)]
pub struct ExpiredVersionRef {
    pub key: String,
    pub version_id: String,
}

pub async fn put_bucket_lifecycle(
    ctx: &DbContext,
    bucket: &str,
    rules: &[LifecycleRule],
) -> Result<(), StorageError> {
    validate_bucket_name(bucket)?;
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    replace_lifecycle_rules(&mut conn, bucket_id, rules).await
}

pub async fn get_bucket_lifecycle(
    ctx: &DbContext,
    bucket: &str,
) -> Result<Vec<LifecycleRule>, StorageError> {
    validate_bucket_name(bucket)?;
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    load_lifecycle_rules(&mut conn, bucket_id).await
}

pub async fn delete_bucket_lifecycle(ctx: &DbContext, bucket: &str) -> Result<(), StorageError> {
    validate_bucket_name(bucket)?;
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    diesel::delete(
        bucket_lifecycle_rules::table.filter(bucket_lifecycle_rules::bucket_id.eq(bucket_id)),
    )
    .execute(&mut conn)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn list_buckets_with_lifecycle(
    ctx: &DbContext,
) -> Result<Vec<BucketLifecycleEntry>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;

    let rows: Vec<(Uuid, String)> = bucket_lifecycle_rules::table
        .inner_join(buckets::table)
        .filter(bucket_lifecycle_rules::enabled.eq(true))
        .select((buckets::id, buckets::name))
        .distinct()
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let mut out = Vec::with_capacity(rows.len());
    for (bucket_id, bucket_name) in rows {
        let rules = load_lifecycle_rules(&mut conn, bucket_id).await?;
        if rules.iter().any(|r| r.enabled) {
            out.push(BucketLifecycleEntry {
                bucket_name,
                bucket_id,
                rules,
            });
        }
    }
    Ok(out)
}

pub async fn list_expired_current_objects(
    ctx: &DbContext,
    bucket_id: Uuid,
    prefix: &str,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<String>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let mut query = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::last_modified.lt(cutoff))
        .filter(objects::is_delete_marker.eq(false))
        .filter(objects::is_folder_marker.eq(false))
        .into_boxed();

    if !prefix.is_empty() {
        let pattern = format!("{}%", escape_like(prefix));
        query = query.filter(objects::key.like(pattern));
    }

    query
        .select(objects::key)
        .order(objects::key.asc())
        .limit(limit)
        .load::<String>(&mut conn)
        .await
        .map_err(db_err)
}

pub async fn list_expired_noncurrent_versions(
    ctx: &DbContext,
    bucket_id: Uuid,
    prefix: &str,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<ExpiredVersionRef>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let mut query = object_versions::table
        .filter(object_versions::bucket_id.eq(bucket_id))
        .filter(object_versions::is_current.eq(false))
        .filter(object_versions::is_delete_marker.eq(false))
        .filter(object_versions::is_folder_marker.eq(false))
        .filter(object_versions::noncurrent_since.is_not_null())
        .filter(object_versions::noncurrent_since.lt(cutoff))
        .into_boxed();

    if !prefix.is_empty() {
        let pattern = format!("{}%", escape_like(prefix));
        query = query.filter(object_versions::key.like(pattern));
    }

    let rows: Vec<(String, String)> = query
        .select((object_versions::key, object_versions::version_id))
        .order((
            object_versions::key.asc(),
            object_versions::version_id.asc(),
        ))
        .limit(limit)
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    Ok(rows
        .into_iter()
        .map(|(key, version_id)| ExpiredVersionRef { key, version_id })
        .collect())
}

async fn load_lifecycle_rules(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
) -> Result<Vec<LifecycleRule>, StorageError> {
    let rows: Vec<(String, bool, String, JsonValue, i32)> = bucket_lifecycle_rules::table
        .filter(bucket_lifecycle_rules::bucket_id.eq(bucket_id))
        .order(bucket_lifecycle_rules::sort_order.asc())
        .select((
            bucket_lifecycle_rules::rule_id,
            bucket_lifecycle_rules::enabled,
            bucket_lifecycle_rules::prefix,
            bucket_lifecycle_rules::actions,
            bucket_lifecycle_rules::sort_order,
        ))
        .load(conn)
        .await
        .map_err(db_err)?;

    rows.into_iter()
        .map(|(id, enabled, prefix, actions_json, _)| {
            let actions: Vec<LifecycleAction> =
                serde_json::from_value(actions_json).map_err(|e| db_err(e.to_string()))?;
            let prefix = if prefix.is_empty() {
                None
            } else {
                Some(prefix)
            };
            Ok(LifecycleRule {
                id,
                enabled,
                prefix,
                actions,
            })
        })
        .collect()
}

async fn replace_lifecycle_rules(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
    rules: &[LifecycleRule],
) -> Result<(), StorageError> {
    diesel::delete(
        bucket_lifecycle_rules::table.filter(bucket_lifecycle_rules::bucket_id.eq(bucket_id)),
    )
    .execute(conn)
    .await
    .map_err(db_err)?;

    if rules.is_empty() {
        return Ok(());
    }

    let rows: Vec<_> = rules
        .iter()
        .enumerate()
        .map(|(idx, rule)| {
            let actions_json =
                serde_json::to_value(&rule.actions).map_err(|e| db_err(e.to_string()))?;
            Ok((
                bucket_lifecycle_rules::id.eq(Uuid::new_v4()),
                bucket_lifecycle_rules::bucket_id.eq(bucket_id),
                bucket_lifecycle_rules::rule_id.eq(&rule.id),
                bucket_lifecycle_rules::enabled.eq(rule.enabled),
                bucket_lifecycle_rules::prefix.eq(rule.prefix.as_deref().unwrap_or("")),
                bucket_lifecycle_rules::actions.eq(actions_json),
                bucket_lifecycle_rules::sort_order.eq(idx as i32),
            ))
        })
        .collect::<Result<Vec<_>, StorageError>>()?;

    diesel::insert_into(bucket_lifecycle_rules::table)
        .values(rows)
        .execute(conn)
        .await
        .map_err(db_err)?;
    Ok(())
}
