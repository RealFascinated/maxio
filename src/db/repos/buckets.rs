use std::collections::HashMap;

use crate::db::schema::{bucket_acl_grants, bucket_cors_rules, bucket_policies, buckets, objects};
use crate::db::{CachedBucketEntry, DbContext};
use crate::iam::Acl;
use crate::storage::{BucketMeta, CorsRule, StorageError, validate_bucket_name};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use super::{
    AclGrantRow, BucketAuthSnapshot, PutBucketContext, db_err, encode_grantee, format_ts, get_conn,
    grants_to_acl, parse_ts, permission_to_db, resolve_bucket_id,
};

type BucketListRow = (Uuid, String, chrono::DateTime<Utc>, bool, String, String);

type CorsRuleRow = (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Option<i32>,
);

pub async fn create_bucket(ctx: &DbContext, meta: &BucketMeta) -> Result<bool, StorageError> {
    validate_bucket_name(&meta.name)?;
    let mut conn = get_conn(ctx.pool()).await?;

    let exists = diesel::select(diesel::dsl::exists(
        buckets::table.filter(buckets::name.eq(&meta.name)),
    ))
    .get_result::<bool>(&mut conn)
    .await
    .map_err(db_err)?;

    if exists {
        return Ok(false);
    }

    let bucket_id = Uuid::new_v4();
    let created_at = parse_ts(&meta.created_at)?;

    diesel::insert_into(buckets::table)
        .values((
            buckets::id.eq(bucket_id),
            buckets::name.eq(&meta.name),
            buckets::created_at.eq(created_at),
            buckets::versioning.eq(meta.versioning),
            buckets::owner_id.eq(&meta.owner_id),
            buckets::owner_display_name.eq(&meta.owner_display_name),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;

    if let Some(ref policy) = meta.policy {
        diesel::insert_into(bucket_policies::table)
            .values((
                bucket_policies::bucket_id.eq(bucket_id),
                bucket_policies::document.eq(policy),
            ))
            .execute(&mut conn)
            .await
            .map_err(db_err)?;
    }

    if let Some(ref rules) = meta.cors_rules {
        replace_cors_rules(&mut conn, bucket_id, rules).await?;
    }

    if let Some(ref acl) = meta.acl {
        replace_bucket_acl(&mut conn, bucket_id, acl).await?;
    }

    ctx.bucket_cache().insert(
        &meta.name,
        CachedBucketEntry {
            id: bucket_id,
            versioning: meta.versioning,
            versioning_suspended: false,
            owner_id: meta.owner_id.clone(),
            owner_display_name: meta.owner_display_name.clone(),
            policy: meta.policy.clone(),
            acl: meta.acl.clone(),
            cors_rules: meta.cors_rules.clone().unwrap_or_default(),
            cors_loaded: true,
        },
    );
    Ok(true)
}

pub async fn head_bucket(ctx: &DbContext, name: &str) -> Result<bool, StorageError> {
    validate_bucket_name(name)?;
    if ctx.bucket_cache().get(name).is_some() {
        return Ok(true);
    }
    ctx.bucket_cache().record_miss();
    let mut conn = get_conn(ctx.pool()).await?;
    diesel::select(diesel::dsl::exists(
        buckets::table.filter(buckets::name.eq(name)),
    ))
    .get_result::<bool>(&mut conn)
    .await
    .map_err(db_err)
}

pub async fn delete_bucket(ctx: &DbContext, name: &str) -> Result<bool, StorageError> {
    validate_bucket_name(name)?;
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = match resolve_bucket_id(ctx.bucket_cache(), &mut conn, name).await {
        Ok(id) => id,
        Err(StorageError::NotFound(_)) => return Ok(false),
        Err(e) => return Err(e),
    };

    let count: i64 = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .count()
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;

    if count > 0 {
        return Err(StorageError::BucketNotEmpty);
    }

    let deleted = diesel::delete(buckets::table.filter(buckets::id.eq(bucket_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;

    if deleted > 0 {
        ctx.bucket_cache().remove(name);
        ctx.object_read_cache().remove_bucket(name);
    }
    Ok(deleted > 0)
}

pub async fn list_buckets(ctx: &DbContext) -> Result<Vec<BucketMeta>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let rows: Vec<BucketListRow> = buckets::table
        .select((
            buckets::id,
            buckets::name,
            buckets::created_at,
            buckets::versioning,
            buckets::owner_id,
            buckets::owner_display_name,
        ))
        .order(buckets::name.asc())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    if rows.is_empty() {
        return Ok(vec![]);
    }

    // Batch-load policies and ACL grants in two queries instead of 4N per bucket.
    let ids: Vec<Uuid> = rows.iter().map(|(id, ..)| *id).collect();

    let policy_rows: Vec<(Uuid, String)> = bucket_policies::table
        .filter(bucket_policies::bucket_id.eq_any(&ids))
        .select((bucket_policies::bucket_id, bucket_policies::document))
        .load(&mut conn)
        .await
        .map_err(db_err)?;
    let mut policies: HashMap<Uuid, String> = policy_rows.into_iter().collect();

    type BucketAclRow = (
        Uuid,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    );
    let raw_acl_rows: Vec<BucketAclRow> = bucket_acl_grants::table
        .filter(bucket_acl_grants::bucket_id.eq_any(&ids))
        .select((
            bucket_acl_grants::bucket_id,
            bucket_acl_grants::grantee_type,
            bucket_acl_grants::grantee_id,
            bucket_acl_grants::grantee_uri,
            bucket_acl_grants::grantee_display_name,
            bucket_acl_grants::permission,
        ))
        .load(&mut conn)
        .await
        .map_err(db_err)?;
    let mut acl_by_bucket: HashMap<Uuid, Vec<AclGrantRow>> = HashMap::new();
    for (bucket_id, gt, gid, guri, gdn, perm) in raw_acl_rows {
        acl_by_bucket
            .entry(bucket_id)
            .or_default()
            .push((gt, gid, guri, gdn, perm));
    }

    let mut result = Vec::with_capacity(rows.len());
    for (id, name, created_at, versioning, owner_id, owner_display_name) in rows {
        let policy = policies.remove(&id);
        let acl = match acl_by_bucket.remove(&id) {
            Some(grants) if !grants.is_empty() => {
                Some(grants_to_acl(&owner_id, &owner_display_name, &grants)?)
            }
            _ => None,
        };
        result.push(BucketMeta {
            name,
            created_at: format_ts(created_at),
            versioning,
            cors_rules: None,
            owner_id,
            owner_display_name,
            acl,
            policy,
        });
    }
    Ok(result)
}

pub async fn put_bucket_policy(
    ctx: &DbContext,
    bucket: &str,
    policy: &str,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;

    diesel::insert_into(bucket_policies::table)
        .values((
            bucket_policies::bucket_id.eq(bucket_id),
            bucket_policies::document.eq(policy),
        ))
        .on_conflict(bucket_policies::bucket_id)
        .do_update()
        .set(bucket_policies::document.eq(policy))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    ctx.bucket_cache()
        .set_policy(bucket, Some(policy.to_string()));
    Ok(())
}

pub async fn get_bucket_policy(
    ctx: &DbContext,
    bucket: &str,
) -> Result<Option<String>, StorageError> {
    validate_bucket_name(bucket)?;
    if let Some(entry) = ctx.bucket_cache().get(bucket) {
        return Ok(entry.policy);
    }
    ctx.bucket_cache().record_miss();
    let mut conn = get_conn(ctx.pool()).await?;
    let entry = load_bucket_cache_entry(&mut conn, bucket).await?;
    ctx.bucket_cache().insert(bucket, entry.clone());
    Ok(entry.policy)
}

pub async fn delete_bucket_policy(ctx: &DbContext, bucket: &str) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    diesel::delete(bucket_policies::table.filter(bucket_policies::bucket_id.eq(bucket_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    ctx.bucket_cache().set_policy(bucket, None);
    Ok(())
}

pub async fn put_bucket_acl(ctx: &DbContext, bucket: &str, acl: Acl) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    replace_bucket_acl(&mut conn, bucket_id, &acl).await?;
    ctx.bucket_cache().set_acl(bucket, Some(acl));
    Ok(())
}

pub async fn get_bucket_acl(ctx: &DbContext, bucket: &str) -> Result<Acl, StorageError> {
    validate_bucket_name(bucket)?;
    if let Some(entry) = ctx.bucket_cache().get(bucket) {
        return Ok(entry
            .acl
            .unwrap_or_else(|| Acl::private(&entry.owner_id, &entry.owner_display_name)));
    }
    ctx.bucket_cache().record_miss();
    let mut conn = get_conn(ctx.pool()).await?;
    let entry = load_bucket_cache_entry(&mut conn, bucket).await?;
    ctx.bucket_cache().insert(bucket, entry.clone());
    Ok(entry
        .acl
        .unwrap_or_else(|| Acl::private(&entry.owner_id, &entry.owner_display_name)))
}

pub async fn put_bucket_cors(
    ctx: &DbContext,
    bucket: &str,
    rules: Vec<CorsRule>,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    replace_cors_rules(&mut conn, bucket_id, &rules).await?;
    ctx.bucket_cache().set_cors(bucket, rules);
    Ok(())
}

pub async fn get_bucket_cors(
    ctx: &DbContext,
    bucket: &str,
) -> Result<Option<Vec<CorsRule>>, StorageError> {
    validate_bucket_name(bucket)?;
    if let Some(entry) = ctx.bucket_cache().get(bucket) {
        if entry.cors_loaded {
            return Ok(Some(entry.cors_rules));
        }
    } else {
        ctx.bucket_cache().record_miss();
    }
    let mut conn = get_conn(ctx.pool()).await?;
    let entry = load_bucket_cache_entry(&mut conn, bucket).await?;
    ctx.bucket_cache().insert(bucket, entry.clone());
    Ok(Some(entry.cors_rules))
}

pub async fn delete_bucket_cors(ctx: &DbContext, bucket: &str) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    diesel::delete(bucket_cors_rules::table.filter(bucket_cors_rules::bucket_id.eq(bucket_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    ctx.bucket_cache().set_cors(bucket, vec![]);
    Ok(())
}

/// PutObject bucket fields (process-local cache, then one DB round-trip).
pub async fn fetch_put_bucket_context(
    ctx: &DbContext,
    bucket: &str,
) -> Result<PutBucketContext, StorageError> {
    validate_bucket_name(bucket)?;
    if let Some(entry) = ctx.bucket_cache().get(bucket) {
        return Ok(entry.into());
    }

    ctx.bucket_cache().record_miss();
    let started = crate::perf::start();
    let mut conn = get_conn(ctx.pool()).await?;
    let entry = load_bucket_cache_entry_core(&mut conn, bucket).await?;
    ctx.bucket_cache().insert(bucket, entry.clone());
    crate::perf::done_detail("fetch_put_bucket_context", started, bucket);
    Ok(entry.into())
}

/// Policy + ACL for authorization (cached; skips CORS).
pub async fn fetch_bucket_auth_context(
    ctx: &DbContext,
    bucket: &str,
) -> Result<BucketAuthSnapshot, StorageError> {
    validate_bucket_name(bucket)?;
    if let Some(entry) = ctx.bucket_cache().get(bucket) {
        return Ok(entry.into());
    }

    ctx.bucket_cache().record_miss();
    let mut conn = get_conn(ctx.pool()).await?;
    let entry = load_bucket_cache_entry_core(&mut conn, bucket).await?;
    ctx.bucket_cache().insert(bucket, entry.clone());
    Ok(entry.into())
}

pub(crate) async fn load_bucket_cache_entry(
    conn: &mut diesel_async::AsyncPgConnection,
    name: &str,
) -> Result<CachedBucketEntry, StorageError> {
    let mut entry = load_bucket_cache_entry_core(conn, name).await?;
    entry.cors_rules = load_bucket_cors_rules(conn, entry.id).await?;
    entry.cors_loaded = true;
    Ok(entry)
}

async fn load_bucket_cache_entry_core(
    conn: &mut diesel_async::AsyncPgConnection,
    name: &str,
) -> Result<CachedBucketEntry, StorageError> {
    validate_bucket_name(name)?;
    let row: (Uuid, bool, bool, String, String) = buckets::table
        .filter(buckets::name.eq(name))
        .select((
            buckets::id,
            buckets::versioning,
            buckets::versioning_suspended,
            buckets::owner_id,
            buckets::owner_display_name,
        ))
        .first(conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::NotFound(name.to_string()),
            other => db_err(other),
        })?;

    let (policy, acl) = load_bucket_auth_parts(conn, row.0, &row.3, &row.4).await?;

    Ok(CachedBucketEntry {
        id: row.0,
        versioning: row.1,
        versioning_suspended: row.2,
        owner_id: row.3,
        owner_display_name: row.4,
        policy,
        acl,
        cors_rules: Vec::new(),
        cors_loaded: false,
    })
}

async fn load_bucket_cors_rules(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
) -> Result<Vec<CorsRule>, StorageError> {
    let cors_rows: Vec<CorsRuleRow> = bucket_cors_rules::table
        .filter(bucket_cors_rules::bucket_id.eq(bucket_id))
        .select((
            bucket_cors_rules::allowed_origins,
            bucket_cors_rules::allowed_methods,
            bucket_cors_rules::allowed_headers,
            bucket_cors_rules::expose_headers,
            bucket_cors_rules::max_age_seconds,
        ))
        .load(conn)
        .await
        .map_err(db_err)?;
    Ok(cors_rows_into_rules(cors_rows))
}

pub async fn get_versioning_state(
    ctx: &DbContext,
    bucket: &str,
) -> Result<crate::storage::VersioningState, StorageError> {
    validate_bucket_name(bucket)?;
    let entry = if let Some(entry) = ctx.bucket_cache().get(bucket) {
        entry
    } else {
        ctx.bucket_cache().record_miss();
        let mut conn = get_conn(ctx.pool()).await?;
        let entry = load_bucket_cache_entry(&mut conn, bucket).await?;
        ctx.bucket_cache().insert(bucket, entry.clone());
        entry
    };
    Ok(if entry.versioning {
        crate::storage::VersioningState::Enabled
    } else if entry.versioning_suspended {
        crate::storage::VersioningState::Suspended
    } else {
        crate::storage::VersioningState::Unversioned
    })
}

pub async fn set_versioning_state(
    ctx: &DbContext,
    bucket: &str,
    state: crate::storage::VersioningState,
) -> Result<(), StorageError> {
    validate_bucket_name(bucket)?;
    let (enabled, suspended) = match state {
        crate::storage::VersioningState::Enabled => (true, false),
        crate::storage::VersioningState::Suspended => (false, true),
        crate::storage::VersioningState::Unversioned => (false, false),
    };
    let mut conn = get_conn(ctx.pool()).await?;
    let updated = diesel::update(buckets::table.filter(buckets::name.eq(bucket)))
        .set((
            buckets::versioning.eq(enabled),
            buckets::versioning_suspended.eq(suspended),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;

    if updated == 0 {
        return Err(StorageError::NotFound(bucket.to_string()));
    }
    ctx.bucket_cache()
        .set_versioning_state(bucket, enabled, suspended);
    Ok(())
}

async fn load_bucket_auth_parts(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
    owner_id: &str,
    owner_display_name: &str,
) -> Result<(Option<String>, Option<Acl>), StorageError> {
    let policy = bucket_policies::table
        .filter(bucket_policies::bucket_id.eq(bucket_id))
        .select(bucket_policies::document)
        .first::<String>(conn)
        .await
        .optional()
        .map_err(db_err)?;

    let acl_rows: Vec<AclGrantRow> = bucket_acl_grants::table
        .filter(bucket_acl_grants::bucket_id.eq(bucket_id))
        .select((
            bucket_acl_grants::grantee_type,
            bucket_acl_grants::grantee_id,
            bucket_acl_grants::grantee_uri,
            bucket_acl_grants::grantee_display_name,
            bucket_acl_grants::permission,
        ))
        .load(conn)
        .await
        .map_err(db_err)?;

    let acl = if acl_rows.is_empty() {
        None
    } else {
        Some(grants_to_acl(owner_id, owner_display_name, &acl_rows)?)
    };

    Ok((policy, acl))
}

fn cors_rows_into_rules(rows: Vec<CorsRuleRow>) -> Vec<CorsRule> {
    rows.into_iter()
        .map(|(origins, methods, headers, expose, max_age)| CorsRule {
            allowed_origins: origins,
            allowed_methods: methods,
            allowed_headers: headers,
            expose_headers: expose,
            max_age_seconds: max_age.map(|v| v as u32),
        })
        .collect()
}

async fn replace_cors_rules(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
    rules: &[CorsRule],
) -> Result<(), StorageError> {
    diesel::delete(bucket_cors_rules::table.filter(bucket_cors_rules::bucket_id.eq(bucket_id)))
        .execute(conn)
        .await
        .map_err(db_err)?;

    if !rules.is_empty() {
        let rows: Vec<_> = rules
            .iter()
            .map(|rule| {
                (
                    bucket_cors_rules::id.eq(Uuid::new_v4()),
                    bucket_cors_rules::bucket_id.eq(bucket_id),
                    bucket_cors_rules::allowed_origins.eq(&rule.allowed_origins),
                    bucket_cors_rules::allowed_methods.eq(&rule.allowed_methods),
                    bucket_cors_rules::allowed_headers.eq(&rule.allowed_headers),
                    bucket_cors_rules::expose_headers.eq(&rule.expose_headers),
                    bucket_cors_rules::max_age_seconds.eq(rule.max_age_seconds.map(|v| v as i32)),
                )
            })
            .collect();
        diesel::insert_into(bucket_cors_rules::table)
            .values(rows)
            .execute(conn)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

async fn replace_bucket_acl(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
    acl: &Acl,
) -> Result<(), StorageError> {
    diesel::delete(bucket_acl_grants::table.filter(bucket_acl_grants::bucket_id.eq(bucket_id)))
        .execute(conn)
        .await
        .map_err(db_err)?;

    if !acl.grants.is_empty() {
        let rows: Vec<_> = acl
            .grants
            .iter()
            .map(|grant| {
                let (gt, gid, guri, gdn) = encode_grantee(&grant.grantee);
                (
                    bucket_acl_grants::id.eq(Uuid::new_v4()),
                    bucket_acl_grants::bucket_id.eq(bucket_id),
                    bucket_acl_grants::grantee_type.eq(gt),
                    bucket_acl_grants::grantee_id.eq(gid),
                    bucket_acl_grants::grantee_uri.eq(guri),
                    bucket_acl_grants::grantee_display_name.eq(gdn),
                    bucket_acl_grants::permission.eq(permission_to_db(grant.permission)),
                )
            })
            .collect();
        diesel::insert_into(bucket_acl_grants::table)
            .values(rows)
            .execute(conn)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}
