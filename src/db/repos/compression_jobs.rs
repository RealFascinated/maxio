use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use diesel_async::RunQueryDsl;

use crate::db::DbContext;
use crate::db::schema::{object_versions, objects};
use crate::storage::StorageError;
use crate::storage::compression::already_compressed_key_regex;
use crate::storage::compression_job::{
    CompressionCandidate, CompressionPhase, CompressionState, MIN_REWRITE_SIZE,
};

use super::{db_err, get_conn, resolve_bucket_id};
use crate::storage::validate_bucket_name;

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
    #[diesel(sql_type = BigInt)]
    bytes: i64,
}

/// Number and total size of current objects and versions that are still stored uncompressed.
pub async fn count_compression_candidates(
    ctx: &DbContext,
    bucket: &str,
) -> Result<(i64, i64), StorageError> {
    validate_bucket_name(bucket)?;
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    let row: CountRow = sql_query(format!(
        "SELECT COALESCE(SUM(count), 0)::bigint AS count, COALESCE(SUM(bytes), 0)::bigint AS bytes FROM ( \
         SELECT COUNT(*) AS count, COALESCE(SUM(size), 0) AS bytes FROM objects \
           WHERE bucket_id = $1 AND compressed_size IS NULL AND NOT is_delete_marker \
             AND NOT is_folder_marker AND size >= {MIN_REWRITE_SIZE} AND key !~* $2 \
         UNION ALL \
         SELECT COUNT(*), COALESCE(SUM(size), 0) FROM object_versions \
           WHERE bucket_id = $1 AND compressed_size IS NULL AND NOT is_delete_marker \
             AND NOT is_folder_marker AND size >= {MIN_REWRITE_SIZE} AND key !~* $2) t"
    ))
    .bind::<SqlUuid, _>(bucket_id)
    .bind::<Text, _>(already_compressed_key_regex())
    .get_result(&mut conn)
    .await
    .map_err(db_err)?;
    Ok((row.count, row.bytes))
}

/// Next `limit` uncompressed candidates after the cursor within the current phase.
pub async fn list_compression_candidates(
    ctx: &DbContext,
    bucket: &str,
    state: &CompressionState,
    limit: i64,
) -> Result<Vec<CompressionCandidate>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    match state.phase {
        CompressionPhase::Objects => {
            let mut query = objects::table
                .filter(objects::bucket_id.eq(bucket_id))
                .filter(objects::compressed_size.is_null())
                .filter(objects::is_delete_marker.eq(false))
                .filter(objects::is_folder_marker.eq(false))
                .filter(objects::size.ge(MIN_REWRITE_SIZE))
                .filter(
                    diesel::dsl::sql::<diesel::sql_types::Bool>("key !~* ")
                        .bind::<Text, _>(already_compressed_key_regex()),
                )
                .into_boxed();
            if let Some(after) = &state.cursor_key {
                query = query.filter(objects::key.gt(after));
            }
            let keys: Vec<String> = query
                .order(objects::key.asc())
                .limit(limit)
                .select(objects::key)
                .load(&mut conn)
                .await
                .map_err(db_err)?;
            Ok(keys
                .into_iter()
                .map(|key| CompressionCandidate {
                    key,
                    version_id: None,
                })
                .collect())
        }
        CompressionPhase::Versions => {
            let mut query = object_versions::table
                .filter(object_versions::bucket_id.eq(bucket_id))
                .filter(object_versions::compressed_size.is_null())
                .filter(object_versions::is_delete_marker.eq(false))
                .filter(object_versions::is_folder_marker.eq(false))
                .filter(object_versions::size.ge(MIN_REWRITE_SIZE))
                .filter(
                    diesel::dsl::sql::<diesel::sql_types::Bool>("key !~* ")
                        .bind::<Text, _>(already_compressed_key_regex()),
                )
                .into_boxed();
            if let (Some(key), Some(version)) = (&state.cursor_key, &state.cursor_version) {
                query = query.filter(
                    object_versions::key.gt(key).or(object_versions::key
                        .eq(key)
                        .and(object_versions::version_id.gt(version))),
                );
            }
            let rows: Vec<(String, String)> = query
                .order((
                    object_versions::key.asc(),
                    object_versions::version_id.asc(),
                ))
                .limit(limit)
                .select((object_versions::key, object_versions::version_id))
                .load(&mut conn)
                .await
                .map_err(db_err)?;
            Ok(rows
                .into_iter()
                .map(|(key, version_id)| CompressionCandidate {
                    key,
                    version_id: Some(version_id),
                })
                .collect())
        }
    }
}

/// Records the compressed size of a current object, but only if it is still the same
/// uncompressed payload (`etag` unchanged). Returns whether the row was updated.
pub async fn set_object_compressed_size(
    ctx: &DbContext,
    bucket: &str,
    key: &str,
    etag: &str,
    compressed_size: u64,
) -> Result<bool, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    let updated = diesel::update(
        objects::table
            .filter(objects::bucket_id.eq(bucket_id))
            .filter(objects::key.eq(key))
            .filter(objects::etag.eq(etag))
            .filter(objects::compressed_size.is_null()),
    )
    .set(objects::compressed_size.eq(compressed_size as i64))
    .execute(&mut conn)
    .await
    .map_err(db_err)?;
    if updated > 0 {
        let row: super::objects::ObjectRow = objects::table
            .filter(objects::bucket_id.eq(bucket_id))
            .filter(objects::key.eq(key))
            .select(super::objects::ObjectRow::as_select())
            .first(&mut conn)
            .await
            .map_err(db_err)?;
        ctx.object_read_cache()
            .insert(bucket, key, super::objects::row_into_read_meta(row));
    }
    Ok(updated > 0)
}

/// Version counterpart of [`set_object_compressed_size`].
pub async fn set_version_compressed_size(
    ctx: &DbContext,
    bucket: &str,
    key: &str,
    version_id: &str,
    etag: &str,
    compressed_size: u64,
) -> Result<bool, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    let updated = diesel::update(
        object_versions::table
            .filter(object_versions::bucket_id.eq(bucket_id))
            .filter(object_versions::key.eq(key))
            .filter(object_versions::version_id.eq(version_id))
            .filter(object_versions::etag.eq(etag))
            .filter(object_versions::compressed_size.is_null()),
    )
    .set(object_versions::compressed_size.eq(compressed_size as i64))
    .execute(&mut conn)
    .await
    .map_err(db_err)?;
    Ok(updated > 0)
}

#[derive(QueryableByName)]
struct SampleRow {
    #[diesel(sql_type = Text)]
    key: String,
    #[diesel(sql_type = BigInt)]
    size: i64,
}

/// Random uncompressed current objects, as `(key, size)`, for the compression estimator.
pub async fn sample_compression_candidates(
    ctx: &DbContext,
    bucket: &str,
    limit: i64,
) -> Result<Vec<(String, u64)>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket).await?;
    let rows: Vec<SampleRow> = sql_query(format!(
        "SELECT key, size FROM objects \
         WHERE bucket_id = $1 AND compressed_size IS NULL AND NOT is_delete_marker \
           AND NOT is_folder_marker AND size >= {MIN_REWRITE_SIZE} AND key !~* $3 \
         ORDER BY random() LIMIT $2"
    ))
    .bind::<SqlUuid, _>(bucket_id)
    .bind::<BigInt, _>(limit)
    .bind::<Text, _>(already_compressed_key_regex())
    .load(&mut conn)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(|r| (r.key, r.size as u64)).collect())
}
