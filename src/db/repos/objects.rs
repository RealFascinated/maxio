use std::collections::HashMap;
use std::sync::Arc;

use crate::db::DbContext;
use crate::db::object_read_cache::ReadCacheLookup;
use crate::db::schema::{object_acl_grants, object_checksums, object_tags, objects};
use crate::iam::Acl;
use crate::storage::{ObjectMeta, StorageError};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use super::{
    AclGrantRow, PutBucketContext, checksum_from_db, checksum_to_db, db_err, encode_grantee,
    format_ts, get_conn, grants_to_acl, parse_ts, part_sizes_from_db, part_sizes_to_db,
    permission_to_db, resolve_bucket_id,
};

fn object_has_side_tables(meta: &ObjectMeta) -> bool {
    meta.tags.as_ref().is_some_and(|t| !t.is_empty())
        || meta.acl.is_some()
        || (meta.checksum_algorithm.is_some() && meta.checksum_value.is_some())
}

/// Upsert an object row using a caller-supplied connection and bucket_id.
/// Used by version pointer updates that already hold an open connection.
pub(super) async fn upsert_object_conn(
    ctx: &DbContext,
    bucket_name: &str,
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
    meta: &ObjectMeta,
) -> Result<(), StorageError> {
    let last_modified = parse_ts(&meta.last_modified)?;
    do_upsert_object(conn, bucket_id, meta, last_modified).await?;
    write_through_read_cache(ctx, bucket_name, meta);
    Ok(())
}

fn staged_write_still_current(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
    last_modified: &str,
) -> bool {
    match ctx.object_read_cache().lookup(bucket_name, key) {
        ReadCacheLookup::Absent => false,
        ReadCacheLookup::Hit(cached) => cached.last_modified == last_modified,
        ReadCacheLookup::Miss => true,
    }
}

pub fn defer_object_upsert(
    ctx: &DbContext,
    bucket_name: &str,
    meta: &ObjectMeta,
    put_ctx: Option<PutBucketContext>,
) {
    write_through_read_cache(ctx, bucket_name, meta);

    let ctx = ctx.clone();
    let bucket_name = bucket_name.to_string();
    let meta = meta.clone();
    let staged_at = meta.last_modified.clone();
    let slots = Arc::clone(ctx.async_meta_slots());
    tokio::spawn(async move {
        let started = crate::perf::start();
        let _permit = match slots.acquire().await {
            Ok(p) => p,
            Err(_) => return,
        };
        if !staged_write_still_current(&ctx, &bucket_name, &meta.key, &staged_at) {
            return;
        }
        let result =
            upsert_object_inner(&ctx, &bucket_name, &meta, put_ctx.as_ref(), false).await;
        crate::perf::done_detail("async_upsert_object", started, &bucket_name);
        if let Err(e) = result {
            tracing::warn!(
                bucket = %bucket_name,
                key = %meta.key,
                error = %e,
                "async metadata write failed"
            );
        }
    });
}

pub async fn upsert_object(
    ctx: &DbContext,
    bucket_name: &str,
    meta: &ObjectMeta,
    put_ctx: Option<&PutBucketContext>,
) -> Result<(), StorageError> {
    upsert_object_inner(ctx, bucket_name, meta, put_ctx, true).await
}

async fn upsert_object_inner(
    ctx: &DbContext,
    bucket_name: &str,
    meta: &ObjectMeta,
    put_ctx: Option<&PutBucketContext>,
    refresh_read_cache: bool,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = if let Some(put) = put_ctx {
        put.bucket_id
    } else if let Some(entry) = ctx.bucket_cache().get(bucket_name) {
        entry.id
    } else {
        resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?
    };
    let last_modified = parse_ts(&meta.last_modified)?;
    do_upsert_object(&mut conn, bucket_id, meta, last_modified).await?;
    if refresh_read_cache {
        write_through_read_cache(ctx, bucket_name, meta);
    }
    Ok(())
}

async fn do_upsert_object(
    conn: &mut diesel_async::AsyncPgConnection,
    bucket_id: Uuid,
    meta: &ObjectMeta,
    last_modified: chrono::DateTime<chrono::Utc>,
) -> Result<(), StorageError> {
    let values = (
        objects::id.eq(Uuid::new_v4()),
        objects::bucket_id.eq(bucket_id),
        objects::key.eq(&meta.key),
        objects::size.eq(meta.size as i64),
        objects::etag.eq(&meta.etag),
        objects::content_type.eq(&meta.content_type),
        objects::last_modified.eq(last_modified),
        objects::owner_id.eq(&meta.owner_id),
        objects::owner_display_name.eq(&meta.owner_display_name),
        objects::version_id.eq(&meta.version_id),
        objects::is_delete_marker.eq(meta.is_delete_marker),
        objects::is_folder_marker.eq(meta.key.ends_with('/')),
        objects::part_sizes.eq(part_sizes_to_db(meta.part_sizes.as_deref())),
    );
    let update = (
        objects::size.eq(meta.size as i64),
        objects::etag.eq(&meta.etag),
        objects::content_type.eq(&meta.content_type),
        objects::last_modified.eq(last_modified),
        objects::owner_id.eq(&meta.owner_id),
        objects::owner_display_name.eq(&meta.owner_display_name),
        objects::version_id.eq(&meta.version_id),
        objects::is_delete_marker.eq(meta.is_delete_marker),
        objects::is_folder_marker.eq(meta.key.ends_with('/')),
        objects::part_sizes.eq(part_sizes_to_db(meta.part_sizes.as_deref())),
    );

    if !object_has_side_tables(meta) {
        diesel::insert_into(objects::table)
            .values(values)
            .on_conflict((objects::bucket_id, objects::key))
            .do_update()
            .set(update)
            .execute(conn)
            .await
            .map_err(db_err)?;
        return Ok(());
    }

    let object_id: Uuid = diesel::insert_into(objects::table)
        .values(values)
        .on_conflict((objects::bucket_id, objects::key))
        .do_update()
        .set(update)
        .returning(objects::id)
        .get_result(conn)
        .await
        .map_err(db_err)?;

    if meta.tags.as_ref().is_some_and(|t| !t.is_empty()) {
        replace_object_tags(conn, object_id, meta.tags.as_ref()).await?;
    }
    if meta.acl.is_some() {
        replace_object_acl(conn, object_id, meta.acl.as_ref()).await?;
    }
    if meta.checksum_algorithm.is_some() && meta.checksum_value.is_some() {
        replace_object_checksum(
            conn,
            object_id,
            meta.checksum_algorithm,
            meta.checksum_value.as_deref(),
        )
        .await?;
    }

    Ok(())
}

/// Load object metadata for GET/HEAD without tags or ACL side tables.
pub async fn get_object_for_read(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<ObjectMeta, StorageError> {
    match ctx.object_read_cache().lookup(bucket_name, key) {
        ReadCacheLookup::Hit(meta) => return Ok(*meta),
        ReadCacheLookup::Absent => return Err(StorageError::NotFound(key.to_string())),
        ReadCacheLookup::Miss => {}
    }

    ctx.object_read_cache().record_miss();
    let started = crate::perf::start();
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = if let Some(entry) = ctx.bucket_cache().get(bucket_name) {
        entry.id
    } else {
        resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?
    };

    let row: ObjectReadRow = objects::table
        .left_join(object_checksums::table.on(objects::id.eq(object_checksums::object_id)))
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::key.eq(key))
        .select(ObjectReadRow::as_select())
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => {
                ctx.object_read_cache().mark_absent(bucket_name, key);
                StorageError::NotFound(key.to_string())
            }
            other => db_err(other),
        })?;

    let mut meta = row_into_read_meta(row.object);
    if let (Some(algo), Some(value)) = (row.checksum_algorithm, row.checksum_value) {
        meta.checksum_algorithm = checksum_from_db(&algo);
        meta.checksum_value = Some(value);
    }
    ctx.object_read_cache()
        .insert(bucket_name, key, meta.clone());
    crate::perf::done_detail("get_object_for_read", started, bucket_name);
    Ok(meta)
}

pub async fn get_object_meta(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<ObjectMeta, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let row: ObjectRow = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::key.eq(key))
        .select(ObjectRow::as_select())
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::NotFound(key.to_string()),
            other => db_err(other),
        })?;

    row_into_meta(&mut conn, row).await
}

pub async fn delete_object(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    diesel::delete(
        objects::table
            .filter(objects::bucket_id.eq(bucket_id))
            .filter(objects::key.eq(key)),
    )
    .execute(&mut conn)
    .await
    .map_err(db_err)?;
    ctx.object_read_cache().mark_absent(bucket_name, key);
    Ok(())
}

/// Delete many current-object rows in one round-trip. Returns keys that existed.
pub async fn delete_objects_by_keys(
    ctx: &DbContext,
    bucket_name: &str,
    keys: &[String],
) -> Result<Vec<String>, StorageError> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let deleted_keys: Vec<String> = diesel::delete(
        objects::table
            .filter(objects::bucket_id.eq(bucket_id))
            .filter(objects::key.eq_any(keys)),
    )
    .returning(objects::key)
    .load(&mut conn)
    .await
    .map_err(db_err)?;

    ctx.object_read_cache().mark_absent_many(bucket_name, keys);
    Ok(deleted_keys)
}

pub async fn put_object_acl(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
    acl: Acl,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;
    let object_id: Uuid = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::key.eq(key))
        .select(objects::id)
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::NotFound(key.to_string()),
            other => db_err(other),
        })?;
    replace_object_acl(&mut conn, object_id, Some(&acl)).await
}

pub async fn get_object_acl(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<Acl, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let (object_id, owner_id, owner_display_name): (Uuid, String, String) = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::key.eq(key))
        .select((objects::id, objects::owner_id, objects::owner_display_name))
        .first::<(Uuid, String, String)>(&mut conn)
        .await
        .optional()
        .map_err(db_err)?
        .ok_or_else(|| StorageError::NotFound(key.to_string()))?;

    let acl_rows: Vec<AclGrantRow> = object_acl_grants::table
        .filter(object_acl_grants::object_id.eq(object_id))
        .select((
            object_acl_grants::grantee_type,
            object_acl_grants::grantee_id,
            object_acl_grants::grantee_uri,
            object_acl_grants::grantee_display_name,
            object_acl_grants::permission,
        ))
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    if acl_rows.is_empty() {
        return Ok(Acl::private(&owner_id, &owner_display_name));
    }
    grants_to_acl(&owner_id, &owner_display_name, &acl_rows)
}

pub async fn put_object_tags(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
    tags: HashMap<String, String>,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;
    let object_id: Uuid = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::key.eq(key))
        .select(objects::id)
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::NotFound(key.to_string()),
            other => db_err(other),
        })?;
    let tags_opt = if tags.is_empty() { None } else { Some(tags) };
    replace_object_tags(&mut conn, object_id, tags_opt.as_ref()).await
}

pub async fn get_object_tags(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<HashMap<String, String>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let object_id = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::key.eq(key))
        .select(objects::id)
        .first::<Uuid>(&mut conn)
        .await
        .optional()
        .map_err(db_err)?
        .ok_or_else(|| StorageError::NotFound(key.to_string()))?;

    let tags: Vec<(String, String)> = object_tags::table
        .filter(object_tags::object_id.eq(object_id))
        .select((object_tags::tag_key, object_tags::tag_value))
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    Ok(tags.into_iter().collect())
}

pub async fn delete_object_tags(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<(), StorageError> {
    put_object_tags(ctx, bucket_name, key, HashMap::new()).await
}

fn meta_for_read_cache(meta: &ObjectMeta) -> ObjectMeta {
    ObjectMeta {
        key: meta.key.clone(),
        size: meta.size,
        etag: meta.etag.clone(),
        content_type: meta.content_type.clone(),
        last_modified: meta.last_modified.clone(),
        owner_id: meta.owner_id.clone(),
        owner_display_name: meta.owner_display_name.clone(),
        acl: None,
        version_id: meta.version_id.clone(),
        is_delete_marker: meta.is_delete_marker,
        checksum_algorithm: meta.checksum_algorithm,
        checksum_value: meta.checksum_value.clone(),
        tags: None,
        part_sizes: meta.part_sizes.clone(),
    }
}

fn write_through_read_cache(ctx: &DbContext, bucket_name: &str, meta: &ObjectMeta) {
    ctx.object_read_cache()
        .insert(bucket_name, &meta.key, meta_for_read_cache(meta));
}

pub(crate) fn row_into_read_meta(row: ObjectRow) -> ObjectMeta {
    ObjectMeta {
        key: row.key,
        size: row.size as u64,
        etag: row.etag,
        content_type: row.content_type,
        last_modified: format_ts(row.last_modified),
        owner_id: row.owner_id,
        owner_display_name: row.owner_display_name,
        acl: None,
        version_id: row.version_id,
        is_delete_marker: row.is_delete_marker,
        checksum_algorithm: None,
        checksum_value: None,
        tags: None,
        part_sizes: part_sizes_from_db(row.part_sizes),
    }
}

pub(crate) async fn row_into_meta(
    conn: &mut diesel_async::AsyncPgConnection,
    row: ObjectRow,
) -> Result<ObjectMeta, StorageError> {
    let tags: Vec<(String, String)> = object_tags::table
        .filter(object_tags::object_id.eq(row.id))
        .select((object_tags::tag_key, object_tags::tag_value))
        .load(conn)
        .await
        .map_err(db_err)?;

    let acl_rows: Vec<AclGrantRow> = object_acl_grants::table
        .filter(object_acl_grants::object_id.eq(row.id))
        .select((
            object_acl_grants::grantee_type,
            object_acl_grants::grantee_id,
            object_acl_grants::grantee_uri,
            object_acl_grants::grantee_display_name,
            object_acl_grants::permission,
        ))
        .load(conn)
        .await
        .map_err(db_err)?;

    let checksum: Option<(String, String)> = object_checksums::table
        .filter(object_checksums::object_id.eq(row.id))
        .select((object_checksums::algorithm, object_checksums::value))
        .first(conn)
        .await
        .optional()
        .map_err(db_err)?;

    let acl = if acl_rows.is_empty() {
        None
    } else {
        Some(grants_to_acl(
            &row.owner_id,
            &row.owner_display_name,
            &acl_rows,
        )?)
    };

    let tags_map = if tags.is_empty() {
        None
    } else {
        Some(tags.into_iter().collect())
    };

    let (checksum_algorithm, checksum_value) = match checksum {
        Some((algo, value)) => (checksum_from_db(&algo), Some(value)),
        None => (None, None),
    };

    Ok(ObjectMeta {
        key: row.key,
        size: row.size as u64,
        etag: row.etag,
        content_type: row.content_type,
        last_modified: format_ts(row.last_modified),
        owner_id: row.owner_id,
        owner_display_name: row.owner_display_name,
        acl,
        version_id: row.version_id,
        is_delete_marker: row.is_delete_marker,
        checksum_algorithm,
        checksum_value,
        tags: tags_map,
        part_sizes: part_sizes_from_db(row.part_sizes),
    })
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = objects)]
pub(crate) struct ObjectRow {
    pub id: Uuid,
    pub key: String,
    pub size: i64,
    pub etag: String,
    pub content_type: String,
    pub last_modified: chrono::DateTime<Utc>,
    pub owner_id: String,
    pub owner_display_name: String,
    pub version_id: Option<String>,
    pub is_delete_marker: bool,
    pub part_sizes: Option<Vec<i64>>,
}

#[derive(Queryable, Selectable)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct ObjectReadRow {
    #[diesel(embed)]
    object: ObjectRow,
    #[diesel(select_expression = object_checksums::algorithm.nullable())]
    checksum_algorithm: Option<String>,
    #[diesel(select_expression = object_checksums::value.nullable())]
    checksum_value: Option<String>,
}

async fn replace_object_tags(
    conn: &mut diesel_async::AsyncPgConnection,
    object_id: Uuid,
    tags: Option<&HashMap<String, String>>,
) -> Result<(), StorageError> {
    diesel::delete(object_tags::table.filter(object_tags::object_id.eq(object_id)))
        .execute(conn)
        .await
        .map_err(db_err)?;

    if let Some(tags) = tags {
        if !tags.is_empty() {
            let rows: Vec<_> = tags
                .iter()
                .map(|(k, v)| {
                    (
                        object_tags::object_id.eq(object_id),
                        object_tags::tag_key.eq(k.as_str()),
                        object_tags::tag_value.eq(v.as_str()),
                    )
                })
                .collect();
            diesel::insert_into(object_tags::table)
                .values(rows)
                .execute(conn)
                .await
                .map_err(db_err)?;
        }
    }
    Ok(())
}

async fn replace_object_acl(
    conn: &mut diesel_async::AsyncPgConnection,
    object_id: Uuid,
    acl: Option<&Acl>,
) -> Result<(), StorageError> {
    diesel::delete(object_acl_grants::table.filter(object_acl_grants::object_id.eq(object_id)))
        .execute(conn)
        .await
        .map_err(db_err)?;

    if let Some(acl) = acl {
        if !acl.grants.is_empty() {
            let rows: Vec<_> = acl
                .grants
                .iter()
                .map(|grant| {
                    let (gt, gid, guri, gdn) = encode_grantee(&grant.grantee);
                    (
                        object_acl_grants::id.eq(Uuid::new_v4()),
                        object_acl_grants::object_id.eq(object_id),
                        object_acl_grants::grantee_type.eq(gt),
                        object_acl_grants::grantee_id.eq(gid),
                        object_acl_grants::grantee_uri.eq(guri),
                        object_acl_grants::grantee_display_name.eq(gdn),
                        object_acl_grants::permission.eq(permission_to_db(grant.permission)),
                    )
                })
                .collect();
            diesel::insert_into(object_acl_grants::table)
                .values(rows)
                .execute(conn)
                .await
                .map_err(db_err)?;
        }
    }
    Ok(())
}

async fn replace_object_checksum(
    conn: &mut diesel_async::AsyncPgConnection,
    object_id: Uuid,
    algorithm: Option<crate::storage::ChecksumAlgorithm>,
    value: Option<&str>,
) -> Result<(), StorageError> {
    diesel::delete(object_checksums::table.filter(object_checksums::object_id.eq(object_id)))
        .execute(conn)
        .await
        .map_err(db_err)?;

    if let (Some(algo), Some(val)) = (algorithm, value) {
        diesel::insert_into(object_checksums::table)
            .values((
                object_checksums::object_id.eq(object_id),
                object_checksums::algorithm.eq(checksum_to_db(algo)),
                object_checksums::value.eq(val),
            ))
            .execute(conn)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}
