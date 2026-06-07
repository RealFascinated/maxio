use crate::db::schema::{
    bucket_acl_grants, bucket_cors_rules, bucket_policies, buckets, objects,
};
use crate::db::{CachedBucketEntry, DbContext};
use crate::iam::Acl;
use crate::storage::{validate_bucket_name, BucketMeta, CorsRule, StorageError};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use super::{
    db_err, encode_grantee, format_ts, get_conn, grants_to_acl, permission_to_db, resolve_bucket_id,
    BucketAuthSnapshot, PutBucketContext,
};

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
    let created_at = parse_created_at(&meta.created_at)?;

    diesel::insert_into(buckets::table)
        .values((
            buckets::id.eq(bucket_id),
            buckets::name.eq(&meta.name),
            buckets::created_at.eq(created_at),
            buckets::region.eq(&meta.region),
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
            owner_id: meta.owner_id.clone(),
            owner_display_name: meta.owner_display_name.clone(),
            policy: meta.policy.clone(),
            acl: meta.acl.clone(),
        },
    );
    Ok(true)
}

pub async fn head_bucket(ctx: &DbContext, name: &str) -> Result<bool, StorageError> {
    validate_bucket_name(name)?;
    if ctx.bucket_cache().get(name).is_some() {
        return Ok(true);
    }
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
    }
    Ok(deleted > 0)
}

pub async fn list_buckets(ctx: &DbContext) -> Result<Vec<BucketMeta>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let rows: Vec<(Uuid, String, chrono::DateTime<Utc>, String, bool, String, String)> =
        buckets::table
            .select((
                buckets::id,
                buckets::name,
                buckets::created_at,
                buckets::region,
                buckets::versioning,
                buckets::owner_id,
                buckets::owner_display_name,
            ))
            .order(buckets::name.asc())
            .load(&mut conn)
            .await
            .map_err(db_err)?;

    let mut result = Vec::with_capacity(rows.len());
    for (id, name, created_at, region, versioning, owner_id, owner_display_name) in rows {
        result.push(
            load_bucket_meta_parts(
                &mut conn,
                id,
                name,
                created_at,
                region,
                versioning,
                owner_id,
                owner_display_name,
            )
            .await?,
        );
    }
    Ok(result)
}

pub async fn get_bucket_meta(ctx: &DbContext, name: &str) -> Result<BucketMeta, StorageError> {
    validate_bucket_name(name)?;
    let mut conn = get_conn(ctx.pool()).await?;
    let row: (Uuid, String, chrono::DateTime<Utc>, String, bool, String, String) = buckets::table
        .filter(buckets::name.eq(name))
        .select((
            buckets::id,
            buckets::name,
            buckets::created_at,
            buckets::region,
            buckets::versioning,
            buckets::owner_id,
            buckets::owner_display_name,
        ))
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::NotFound(name.to_string()),
            other => db_err(other),
        })?;

    load_bucket_meta_parts(
        &mut conn,
        row.0,
        row.1,
        row.2,
        row.3,
        row.4,
        row.5,
        row.6,
    )
    .await
}

pub async fn put_bucket_policy(ctx: &DbContext, bucket: &str, policy: &str) -> Result<(), StorageError> {
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

pub async fn get_bucket_policy(ctx: &DbContext, bucket: &str) -> Result<Option<String>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;

    bucket_policies::table
        .filter(bucket_policies::bucket_id.eq(bucket_id))
        .select(bucket_policies::document)
        .first::<String>(&mut conn)
        .await
        .optional()
        .map_err(db_err)
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
    let meta = get_bucket_meta(ctx, bucket).await?;
    Ok(meta.acl.unwrap_or_else(|| {
        Acl::private(&meta.owner_id, &meta.owner_display_name)
    }))
}

pub async fn put_bucket_cors(
    ctx: &DbContext,
    bucket: &str,
    rules: Vec<CorsRule>,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    replace_cors_rules(&mut conn, bucket_id, &rules).await
}

pub async fn get_bucket_cors(
    ctx: &DbContext,
    bucket: &str,
) -> Result<Option<Vec<CorsRule>>, StorageError> {
    let meta = get_bucket_meta(ctx, bucket).await?;
    Ok(meta.cors_rules)
}

pub async fn delete_bucket_cors(ctx: &DbContext, bucket: &str) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    diesel::delete(bucket_cors_rules::table.filter(bucket_cors_rules::bucket_id.eq(bucket_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
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

    let mut conn = get_conn(ctx.pool()).await?;
    let entry = load_bucket_cache_entry(&mut conn, bucket).await?;
    ctx.bucket_cache().insert(bucket, entry.clone());
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

    let mut conn = get_conn(ctx.pool()).await?;
    let entry = load_bucket_cache_entry(&mut conn, bucket).await?;
    ctx.bucket_cache().insert(bucket, entry.clone());
    Ok(entry.into())
}

pub(crate) async fn load_bucket_cache_entry(
    conn: &mut diesel_async::AsyncPgConnection,
    name: &str,
) -> Result<CachedBucketEntry, StorageError> {
    validate_bucket_name(name)?;
    let row: (Uuid, bool, String, String) = buckets::table
        .filter(buckets::name.eq(name))
        .select((
            buckets::id,
            buckets::versioning,
            buckets::owner_id,
            buckets::owner_display_name,
        ))
        .first(conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::NotFound(name.to_string()),
            other => db_err(other),
        })?;

    let (policy, acl) =
        load_bucket_auth_parts(conn, row.0, &row.2, &row.3).await?;

    Ok(CachedBucketEntry {
        id: row.0,
        versioning: row.1,
        owner_id: row.2,
        owner_display_name: row.3,
        policy,
        acl,
    })
}

pub async fn is_versioned(ctx: &DbContext, bucket: &str) -> Result<bool, StorageError> {
    validate_bucket_name(bucket)?;
    let mut conn = get_conn(ctx.pool()).await?;
    buckets::table
        .filter(buckets::name.eq(bucket))
        .select(buckets::versioning)
        .first::<bool>(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::NotFound(bucket.to_string()),
            other => db_err(other),
        })
}

pub async fn set_versioning(ctx: &DbContext, bucket: &str, enabled: bool) -> Result<(), StorageError> {
    validate_bucket_name(bucket)?;
    let mut conn = get_conn(ctx.pool()).await?;
    let updated = diesel::update(buckets::table.filter(buckets::name.eq(bucket)))
        .set(buckets::versioning.eq(enabled))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;

    if updated == 0 {
        return Err(StorageError::NotFound(bucket.to_string()));
    }
    ctx.bucket_cache().set_versioning(bucket, enabled);
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

    let acl_rows: Vec<(String, Option<String>, Option<String>, Option<String>, String)> =
        bucket_acl_grants::table
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
        Some(grants_to_acl(
            owner_id,
            owner_display_name,
            &acl_rows,
        )?)
    };

    Ok((policy, acl))
}

async fn load_bucket_meta_parts(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
    name: String,
    created_at: chrono::DateTime<Utc>,
    region: String,
    versioning: bool,
    owner_id: String,
    owner_display_name: String,
) -> Result<BucketMeta, StorageError> {
    let (policy, acl) =
        load_bucket_auth_parts(conn, bucket_id, &owner_id, &owner_display_name).await?;

    let cors_rows: Vec<(
        Vec<String>,
        Vec<String>,
        Vec<String>,
        Vec<String>,
        Option<i32>,
    )> = bucket_cors_rules::table
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

    let cors_rules = if cors_rows.is_empty() {
        None
    } else {
        Some(
            cors_rows
                .into_iter()
                .map(
                    |(allowed_origins, allowed_methods, allowed_headers, expose_headers, max_age)| {
                        CorsRule {
                            allowed_origins,
                            allowed_methods,
                            allowed_headers,
                            expose_headers,
                            max_age_seconds: max_age.map(|v| v as u32),
                        }
                    },
                )
                .collect(),
        )
    };

    Ok(BucketMeta {
        name,
        created_at: format_ts(created_at),
        region,
        versioning,
        cors_rules,
        owner_id,
        owner_display_name,
        acl,
        policy,
        public_read: false,
        public_list: false,
    })
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

    for rule in rules {
        diesel::insert_into(bucket_cors_rules::table)
            .values((
                bucket_cors_rules::id.eq(Uuid::new_v4()),
                bucket_cors_rules::bucket_id.eq(bucket_id),
                bucket_cors_rules::allowed_origins.eq(&rule.allowed_origins),
                bucket_cors_rules::allowed_methods.eq(&rule.allowed_methods),
                bucket_cors_rules::allowed_headers.eq(&rule.allowed_headers),
                bucket_cors_rules::expose_headers.eq(&rule.expose_headers),
                bucket_cors_rules::max_age_seconds.eq(rule.max_age_seconds.map(|v| v as i32)),
            ))
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

    for grant in &acl.grants {
        let (gt, gid, guri, gdn) = encode_grantee(&grant.grantee);
        diesel::insert_into(bucket_acl_grants::table)
            .values((
                bucket_acl_grants::id.eq(Uuid::new_v4()),
                bucket_acl_grants::bucket_id.eq(bucket_id),
                bucket_acl_grants::grantee_type.eq(gt),
                bucket_acl_grants::grantee_id.eq(gid),
                bucket_acl_grants::grantee_uri.eq(guri),
                bucket_acl_grants::grantee_display_name.eq(gdn),
                bucket_acl_grants::permission.eq(permission_to_db(grant.permission)),
            ))
            .execute(conn)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

fn parse_created_at(s: &str) -> Result<chrono::DateTime<Utc>, StorageError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.3fZ")
                .map(|ndt| ndt.and_utc())
        })
        .map_err(db_err)
}
