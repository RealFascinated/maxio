use std::collections::HashMap;

use crate::db::DbContext;
use crate::db::schema::{
    object_version_acl_grants, object_version_checksums, object_version_tags, object_versions,
    objects,
};
use crate::storage::{ObjectMeta, StorageError};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use super::{
    AclGrantRow, checksum_from_db, checksum_to_db, db_err, encode_grantee, format_ts, get_conn,
    grants_to_acl, parse_ts, part_sizes_from_db, part_sizes_to_db, permission_to_db,
    resolve_bucket_id,
};

pub async fn insert_version(
    ctx: &DbContext,
    bucket_name: &str,
    meta: &ObjectMeta,
    is_current: bool,
) -> Result<Uuid, StorageError> {
    let version_id = meta
        .version_id
        .as_ref()
        .ok_or_else(|| db_err("version insert requires version_id"))?;

    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;
    let last_modified = parse_ts(&meta.last_modified)?;

    if is_current {
        diesel::update(
            object_versions::table
                .filter(object_versions::bucket_id.eq(bucket_id))
                .filter(object_versions::key.eq(&meta.key)),
        )
        .set(object_versions::is_current.eq(false))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    }

    let row_id: Uuid = diesel::insert_into(object_versions::table)
        .values((
            object_versions::id.eq(Uuid::new_v4()),
            object_versions::bucket_id.eq(bucket_id),
            object_versions::key.eq(&meta.key),
            object_versions::version_id.eq(version_id),
            object_versions::size.eq(meta.size as i64),
            object_versions::etag.eq(&meta.etag),
            object_versions::content_type.eq(&meta.content_type),
            object_versions::last_modified.eq(last_modified),
            object_versions::owner_id.eq(&meta.owner_id),
            object_versions::owner_display_name.eq(&meta.owner_display_name),
            object_versions::is_delete_marker.eq(meta.is_delete_marker),
            object_versions::storage_format.eq(&meta.storage_format),
            object_versions::is_folder_marker.eq(meta.key.ends_with('/')),
            object_versions::part_sizes.eq(part_sizes_to_db(meta.part_sizes.as_deref())),
            object_versions::is_current.eq(is_current),
        ))
        .on_conflict((
            object_versions::bucket_id,
            object_versions::key,
            object_versions::version_id,
        ))
        .do_update()
        .set((
            object_versions::size.eq(meta.size as i64),
            object_versions::etag.eq(&meta.etag),
            object_versions::content_type.eq(&meta.content_type),
            object_versions::last_modified.eq(last_modified),
            object_versions::owner_id.eq(&meta.owner_id),
            object_versions::owner_display_name.eq(&meta.owner_display_name),
            object_versions::is_delete_marker.eq(meta.is_delete_marker),
            object_versions::storage_format.eq(&meta.storage_format),
            object_versions::is_folder_marker.eq(meta.key.ends_with('/')),
            object_versions::part_sizes.eq(part_sizes_to_db(meta.part_sizes.as_deref())),
            object_versions::is_current.eq(is_current),
        ))
        .returning(object_versions::id)
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;

    replace_version_tags(&mut conn, row_id, meta.tags.as_ref()).await?;
    replace_version_acl(&mut conn, row_id, meta.acl.as_ref()).await?;
    replace_version_checksum(
        &mut conn,
        row_id,
        meta.checksum_algorithm,
        meta.checksum_value.as_deref(),
    )
    .await?;

    Ok(row_id)
}

pub async fn get_object_version_meta(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
    version_id: &str,
) -> Result<ObjectMeta, StorageError> {
    if version_id == "null" {
        return super::get_object_meta(ctx, bucket_name, key).await;
    }

    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let row: VersionRow = object_versions::table
        .filter(object_versions::bucket_id.eq(bucket_id))
        .filter(object_versions::key.eq(key))
        .filter(object_versions::version_id.eq(version_id))
        .select(VersionRow::as_select())
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => {
                StorageError::VersionNotFound(version_id.to_string())
            }
            other => db_err(other),
        })?;

    version_row_into_meta(&mut conn, row).await
}

pub async fn delete_object_version(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
    version_id: &str,
) -> Result<ObjectMeta, StorageError> {
    if version_id == "null" {
        let meta = super::get_object_meta(ctx, bucket_name, key).await?;
        super::delete_object(ctx, bucket_name, key).await?;
        update_current_after_delete(ctx, bucket_name, key).await?;
        return Ok(meta);
    }

    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let row: VersionRow = object_versions::table
        .filter(object_versions::bucket_id.eq(bucket_id))
        .filter(object_versions::key.eq(key))
        .filter(object_versions::version_id.eq(version_id))
        .select(VersionRow::as_select())
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => {
                StorageError::VersionNotFound(version_id.to_string())
            }
            other => db_err(other),
        })?;

    let meta = version_row_into_meta(&mut conn, row.clone()).await?;

    diesel::delete(
        object_versions::table
            .filter(object_versions::bucket_id.eq(bucket_id))
            .filter(object_versions::key.eq(key))
            .filter(object_versions::version_id.eq(version_id)),
    )
    .execute(&mut conn)
    .await
    .map_err(db_err)?;

    if row.is_current {
        update_current_after_delete(ctx, bucket_name, key).await?;
    }

    Ok(meta)
}

pub async fn list_object_versions(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
) -> Result<Vec<ObjectMeta>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let mut query = object_versions::table
        .filter(object_versions::bucket_id.eq(bucket_id))
        .into_boxed();

    if !prefix.is_empty() {
        let pattern = format!("{}%", super::escape_like(prefix));
        query = query.filter(object_versions::key.like(pattern));
    }

    let version_rows: Vec<VersionRow> = query
        .select(VersionRow::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let mut results = Vec::new();
    for row in version_rows {
        results.push(version_row_into_meta(&mut conn, row).await?);
    }

    // Include current null-version objects (S3 suspended-state compatibility).
    let mut current_query = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::version_id.is_null())
        .into_boxed();

    if !prefix.is_empty() {
        let pattern = format!("{}%", super::escape_like(prefix));
        current_query = current_query.filter(objects::key.like(pattern));
    }

    let current_rows: Vec<super::objects::ObjectRow> = current_query
        .select(super::objects::ObjectRow::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    for row in current_rows {
        results.push(super::objects::row_into_meta(&mut conn, row).await?);
    }

    results.sort_by(|a, b| {
        a.key.cmp(&b.key).then_with(|| {
            let va = a.version_id.as_deref().unwrap_or("");
            let vb = b.version_id.as_deref().unwrap_or("");
            vb.cmp(va)
        })
    });

    Ok(results)
}

pub async fn update_current_after_delete(
    ctx: &DbContext,
    bucket_name: &str,
    key: &str,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let latest: Option<VersionRow> = object_versions::table
        .filter(object_versions::bucket_id.eq(bucket_id))
        .filter(object_versions::key.eq(key))
        .filter(object_versions::is_delete_marker.eq(false))
        .order(object_versions::version_id.desc())
        .select(VersionRow::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;

    diesel::delete(
        objects::table
            .filter(objects::bucket_id.eq(bucket_id))
            .filter(objects::key.eq(key)),
    )
    .execute(&mut conn)
    .await
    .map_err(db_err)?;

    diesel::update(
        object_versions::table
            .filter(object_versions::bucket_id.eq(bucket_id))
            .filter(object_versions::key.eq(key)),
    )
    .set(object_versions::is_current.eq(false))
    .execute(&mut conn)
    .await
    .map_err(db_err)?;

    if let Some(row) = latest {
        let meta = version_row_into_meta(&mut conn, row.clone()).await?;
        super::upsert_object(ctx, bucket_name, &meta, None).await?;
        diesel::update(object_versions::table.filter(object_versions::id.eq(row.id)))
            .set(object_versions::is_current.eq(true))
            .execute(&mut conn)
            .await
            .map_err(db_err)?;
    }

    Ok(())
}

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = object_versions)]
struct VersionRow {
    id: Uuid,
    key: String,
    version_id: String,
    #[allow(dead_code)]
    is_current: bool,
    size: i64,
    etag: String,
    content_type: String,
    last_modified: chrono::DateTime<Utc>,
    owner_id: String,
    owner_display_name: String,
    is_delete_marker: bool,
    storage_format: Option<String>,
    part_sizes: Option<Vec<i64>>,
}

async fn version_row_into_meta(
    conn: &mut diesel_async::AsyncPgConnection,
    row: VersionRow,
) -> Result<ObjectMeta, StorageError> {
    let tags: Vec<(String, String)> = object_version_tags::table
        .filter(object_version_tags::object_version_id.eq(row.id))
        .select((object_version_tags::tag_key, object_version_tags::tag_value))
        .load(conn)
        .await
        .map_err(db_err)?;

    let acl_rows: Vec<AclGrantRow> = object_version_acl_grants::table
        .filter(object_version_acl_grants::object_version_id.eq(row.id))
        .select((
            object_version_acl_grants::grantee_type,
            object_version_acl_grants::grantee_id,
            object_version_acl_grants::grantee_uri,
            object_version_acl_grants::grantee_display_name,
            object_version_acl_grants::permission,
        ))
        .load(conn)
        .await
        .map_err(db_err)?;

    let checksum: Option<(String, String)> = object_version_checksums::table
        .filter(object_version_checksums::object_version_id.eq(row.id))
        .select((
            object_version_checksums::algorithm,
            object_version_checksums::value,
        ))
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
        version_id: Some(row.version_id),
        is_delete_marker: row.is_delete_marker,
        storage_format: row.storage_format,
        checksum_algorithm,
        checksum_value,
        tags: tags_map,
        part_sizes: part_sizes_from_db(row.part_sizes),
    })
}

async fn replace_version_tags(
    conn: &mut diesel_async::AsyncPgConnection,
    version_id: Uuid,
    tags: Option<&HashMap<String, String>>,
) -> Result<(), StorageError> {
    diesel::delete(
        object_version_tags::table.filter(object_version_tags::object_version_id.eq(version_id)),
    )
    .execute(conn)
    .await
    .map_err(db_err)?;

    if let Some(tags) = tags {
        for (tag_key, tag_value) in tags {
            diesel::insert_into(object_version_tags::table)
                .values((
                    object_version_tags::object_version_id.eq(version_id),
                    object_version_tags::tag_key.eq(tag_key),
                    object_version_tags::tag_value.eq(tag_value),
                ))
                .execute(conn)
                .await
                .map_err(db_err)?;
        }
    }
    Ok(())
}

async fn replace_version_acl(
    conn: &mut diesel_async::AsyncPgConnection,
    version_id: Uuid,
    acl: Option<&crate::iam::Acl>,
) -> Result<(), StorageError> {
    diesel::delete(
        object_version_acl_grants::table
            .filter(object_version_acl_grants::object_version_id.eq(version_id)),
    )
    .execute(conn)
    .await
    .map_err(db_err)?;

    if let Some(acl) = acl {
        for grant in &acl.grants {
            let (gt, gid, guri, gdn) = encode_grantee(&grant.grantee);
            diesel::insert_into(object_version_acl_grants::table)
                .values((
                    object_version_acl_grants::id.eq(Uuid::new_v4()),
                    object_version_acl_grants::object_version_id.eq(version_id),
                    object_version_acl_grants::grantee_type.eq(gt),
                    object_version_acl_grants::grantee_id.eq(gid),
                    object_version_acl_grants::grantee_uri.eq(guri),
                    object_version_acl_grants::grantee_display_name.eq(gdn),
                    object_version_acl_grants::permission.eq(permission_to_db(grant.permission)),
                ))
                .execute(conn)
                .await
                .map_err(db_err)?;
        }
    }
    Ok(())
}

async fn replace_version_checksum(
    conn: &mut diesel_async::AsyncPgConnection,
    version_id: Uuid,
    algorithm: Option<crate::storage::ChecksumAlgorithm>,
    value: Option<&str>,
) -> Result<(), StorageError> {
    diesel::delete(
        object_version_checksums::table
            .filter(object_version_checksums::object_version_id.eq(version_id)),
    )
    .execute(conn)
    .await
    .map_err(db_err)?;

    if let (Some(algo), Some(val)) = (algorithm, value) {
        diesel::insert_into(object_version_checksums::table)
            .values((
                object_version_checksums::object_version_id.eq(version_id),
                object_version_checksums::algorithm.eq(checksum_to_db(algo)),
                object_version_checksums::value.eq(val),
            ))
            .execute(conn)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}
