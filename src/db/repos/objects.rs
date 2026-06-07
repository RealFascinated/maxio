use std::collections::HashMap;

use crate::db::schema::{
    object_acl_grants, object_checksums, object_tags, objects,
};
use crate::db::DbContext;
use crate::iam::Acl;
use crate::storage::{ObjectMeta, StorageError};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use super::{
    checksum_from_db, checksum_to_db, db_err, encode_grantee, format_ts, get_conn, grants_to_acl,
    parse_ts, part_sizes_from_db, part_sizes_to_db, permission_to_db, resolve_bucket_id,
    PutBucketContext,
};

fn object_has_side_tables(meta: &ObjectMeta) -> bool {
    meta.tags.as_ref().is_some_and(|t| !t.is_empty())
        || meta.acl.is_some()
        || (meta.checksum_algorithm.is_some() && meta.checksum_value.is_some())
}

pub async fn upsert_object(
    ctx: &DbContext,
    bucket_name: &str,
    meta: &ObjectMeta,
    put_ctx: Option<&PutBucketContext>,
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
        objects::storage_format.eq(&meta.storage_format),
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
        objects::storage_format.eq(&meta.storage_format),
        objects::is_folder_marker.eq(meta.key.ends_with('/')),
        objects::part_sizes.eq(part_sizes_to_db(meta.part_sizes.as_deref())),
    );

    if !object_has_side_tables(meta) {
        diesel::insert_into(objects::table)
            .values(values)
            .on_conflict((objects::bucket_id, objects::key))
            .do_update()
            .set(update)
            .execute(&mut conn)
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
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;

    if meta.tags.as_ref().is_some_and(|t| !t.is_empty()) {
        replace_object_tags(&mut conn, object_id, meta.tags.as_ref()).await?;
    }
    if meta.acl.is_some() {
        replace_object_acl(&mut conn, object_id, meta.acl.as_ref()).await?;
    }
    if meta.checksum_algorithm.is_some() && meta.checksum_value.is_some() {
        replace_object_checksum(
            &mut conn,
            object_id,
            meta.checksum_algorithm,
            meta.checksum_value.as_deref(),
        )
        .await?;
    }

    Ok(())
}

/// Load object metadata for GET/HEAD without tags, ACL, or checksum side tables.
pub async fn get_object_for_read(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<ObjectMeta, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = if let Some(entry) = ctx.bucket_cache().get(bucket_name) {
        entry.id
    } else {
        resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?
    };

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

    Ok(row_into_read_meta(row))
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

pub async fn delete_object(ctx: &DbContext, bucket_name: &str, key: &str) -> Result<(), StorageError> {
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
    Ok(())
}

pub async fn object_exists(ctx: &DbContext, bucket_name: &str, key: &str) -> Result<bool, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;
    diesel::select(diesel::dsl::exists(
        objects::table
            .filter(objects::bucket_id.eq(bucket_id))
            .filter(objects::key.eq(key)),
    ))
    .get_result::<bool>(&mut conn)
    .await
    .map_err(db_err)
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

pub async fn get_object_acl(ctx: &DbContext, bucket_name: &str, key: &str) -> Result<Acl, StorageError> {
    let meta = get_object_meta(ctx, bucket_name, key).await?;
    Ok(meta.acl.unwrap_or_else(|| {
        Acl::private(&meta.owner_id, &meta.owner_display_name)
    }))
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
    let meta = get_object_meta(ctx, bucket_name, key).await?;
    Ok(meta.tags.unwrap_or_default())
}

pub async fn delete_object_tags(ctx: &DbContext, bucket_name: &str, key: &str) -> Result<(), StorageError> {
    put_object_tags(ctx, bucket_name, key, HashMap::new()).await
}

fn row_into_read_meta(row: ObjectRow) -> ObjectMeta {
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
        storage_format: row.storage_format,
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

    let acl_rows: Vec<(String, Option<String>, Option<String>, Option<String>, String)> =
        object_acl_grants::table
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
        storage_format: row.storage_format,
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
    pub storage_format: Option<String>,
    pub part_sizes: Option<Vec<i64>>,
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
        for (tag_key, tag_value) in tags {
            diesel::insert_into(object_tags::table)
                .values((
                    object_tags::object_id.eq(object_id),
                    object_tags::tag_key.eq(tag_key),
                    object_tags::tag_value.eq(tag_value),
                ))
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
        for grant in &acl.grants {
            let (gt, gid, guri, gdn) = encode_grantee(&grant.grantee);
            diesel::insert_into(object_acl_grants::table)
                .values((
                    object_acl_grants::id.eq(Uuid::new_v4()),
                    object_acl_grants::object_id.eq(object_id),
                    object_acl_grants::grantee_type.eq(gt),
                    object_acl_grants::grantee_id.eq(gid),
                    object_acl_grants::grantee_uri.eq(guri),
                    object_acl_grants::grantee_display_name.eq(gdn),
                    object_acl_grants::permission.eq(permission_to_db(grant.permission)),
                ))
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
