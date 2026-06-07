use crate::db::DbContext;
use crate::db::schema::{multipart_parts, multipart_uploads};
use crate::storage::{MultipartUploadMeta, PartMeta, StorageError};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use super::{
    checksum_from_db, checksum_to_db, db_err, format_ts, get_conn, parse_ts, resolve_bucket_id,
};

pub async fn insert_multipart_upload(
    ctx: &DbContext,
    meta: &MultipartUploadMeta,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, &meta.bucket).await?;
    let initiated = parse_ts(&meta.initiated)?;

    diesel::insert_into(multipart_uploads::table)
        .values((
            multipart_uploads::upload_id.eq(&meta.upload_id),
            multipart_uploads::bucket_id.eq(bucket_id),
            multipart_uploads::key.eq(&meta.key),
            multipart_uploads::content_type.eq(&meta.content_type),
            multipart_uploads::initiated.eq(initiated),
            multipart_uploads::checksum_algorithm.eq(meta.checksum_algorithm.map(checksum_to_db)),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn get_multipart_upload(
    ctx: &DbContext,
    upload_id: &str,
) -> Result<MultipartUploadMeta, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let row: (
        String,
        String,
        String,
        chrono::DateTime<Utc>,
        Option<String>,
    ) = multipart_uploads::table
        .inner_join(crate::db::schema::buckets::table)
        .filter(multipart_uploads::upload_id.eq(upload_id))
        .select((
            crate::db::schema::buckets::name,
            multipart_uploads::key,
            multipart_uploads::content_type,
            multipart_uploads::initiated,
            multipart_uploads::checksum_algorithm,
        ))
        .first(&mut conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => StorageError::UploadNotFound(upload_id.to_string()),
            other => db_err(other),
        })?;

    Ok(MultipartUploadMeta {
        upload_id: upload_id.to_string(),
        bucket: row.0,
        key: row.1,
        content_type: row.2,
        initiated: format_ts(row.3),
        checksum_algorithm: row.4.and_then(|s| checksum_from_db(&s)),
    })
}

pub async fn abort_multipart_upload(ctx: &DbContext, upload_id: &str) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let deleted =
        diesel::delete(multipart_uploads::table.filter(multipart_uploads::upload_id.eq(upload_id)))
            .execute(&mut conn)
            .await
            .map_err(db_err)?;

    if deleted == 0 {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }
    Ok(())
}

pub async fn upsert_part(
    ctx: &DbContext,
    upload_id: &str,
    part: &PartMeta,
) -> Result<(), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;

    let exists = diesel::select(diesel::dsl::exists(
        multipart_uploads::table.filter(multipart_uploads::upload_id.eq(upload_id)),
    ))
    .get_result::<bool>(&mut conn)
    .await
    .map_err(db_err)?;

    if !exists {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }

    let last_modified = parse_ts(&part.last_modified)?;

    diesel::insert_into(multipart_parts::table)
        .values((
            multipart_parts::upload_id.eq(upload_id),
            multipart_parts::part_number.eq(part.part_number as i32),
            multipart_parts::etag.eq(&part.etag),
            multipart_parts::size.eq(part.size as i64),
            multipart_parts::last_modified.eq(last_modified),
            multipart_parts::checksum_algorithm.eq(part.checksum_algorithm.map(checksum_to_db)),
            multipart_parts::checksum_value.eq(&part.checksum_value),
        ))
        .on_conflict((multipart_parts::upload_id, multipart_parts::part_number))
        .do_update()
        .set((
            multipart_parts::etag.eq(&part.etag),
            multipart_parts::size.eq(part.size as i64),
            multipart_parts::last_modified.eq(last_modified),
            multipart_parts::checksum_algorithm.eq(part.checksum_algorithm.map(checksum_to_db)),
            multipart_parts::checksum_value.eq(&part.checksum_value),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn list_parts(
    ctx: &DbContext,
    upload_id: &str,
) -> Result<(MultipartUploadMeta, Vec<PartMeta>), StorageError> {
    let meta = get_multipart_upload(ctx, upload_id).await?;
    let mut conn = get_conn(ctx.pool()).await?;

    let rows: Vec<(
        i32,
        String,
        i64,
        chrono::DateTime<Utc>,
        Option<String>,
        Option<String>,
    )> = multipart_parts::table
        .filter(multipart_parts::upload_id.eq(upload_id))
        .order(multipart_parts::part_number.asc())
        .select((
            multipart_parts::part_number,
            multipart_parts::etag,
            multipart_parts::size,
            multipart_parts::last_modified,
            multipart_parts::checksum_algorithm,
            multipart_parts::checksum_value,
        ))
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let parts = rows
        .into_iter()
        .map(
            |(part_number, etag, size, last_modified, algo, value)| PartMeta {
                part_number: part_number as u32,
                etag,
                size: size as u64,
                last_modified: format_ts(last_modified),
                checksum_algorithm: algo.and_then(|s| checksum_from_db(&s)),
                checksum_value: value,
            },
        )
        .collect();

    Ok((meta, parts))
}

pub async fn list_multipart_uploads(
    ctx: &DbContext,
    bucket_name: &str,
) -> Result<Vec<MultipartUploadMeta>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let rows: Vec<(
        String,
        String,
        String,
        chrono::DateTime<Utc>,
        Option<String>,
    )> = multipart_uploads::table
        .filter(multipart_uploads::bucket_id.eq(bucket_id))
        .order(multipart_uploads::initiated.asc())
        .select((
            multipart_uploads::upload_id,
            multipart_uploads::key,
            multipart_uploads::content_type,
            multipart_uploads::initiated,
            multipart_uploads::checksum_algorithm,
        ))
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    Ok(rows
        .into_iter()
        .map(
            |(upload_id, key, content_type, initiated, algo)| MultipartUploadMeta {
                upload_id,
                bucket: bucket_name.to_string(),
                key,
                content_type,
                initiated: format_ts(initiated),
                checksum_algorithm: algo.and_then(|s| checksum_from_db(&s)),
            },
        )
        .collect())
}

/// Remove multipart uploads older than `stale_after`. Returns count removed.
pub async fn cleanup_stale_uploads(
    ctx: &DbContext,
    stale_after: chrono::Duration,
) -> Result<u64, StorageError> {
    let cutoff = Utc::now() - stale_after;
    let mut conn = get_conn(ctx.pool()).await?;

    let stale_ids: Vec<String> = multipart_uploads::table
        .filter(multipart_uploads::initiated.lt(cutoff))
        .select(multipart_uploads::upload_id)
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    if stale_ids.is_empty() {
        return Ok(0);
    }

    let removed = diesel::delete(
        multipart_uploads::table.filter(multipart_uploads::upload_id.eq_any(&stale_ids)),
    )
    .execute(&mut conn)
    .await
    .map_err(db_err)?;

    Ok(removed as u64)
}
