use crate::db::schema::objects;
use crate::db::DbContext;
use crate::storage::{ObjectMeta, StorageError};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use super::{db_err, get_conn, resolve_bucket_id};
use super::objects::{row_into_meta, ObjectRow};

pub async fn list_objects_page(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    start_after: Option<&str>,
    max_keys: usize,
) -> Result<(Vec<ObjectMeta>, bool, Option<String>), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let limit = max_keys.saturating_add(1) as i64;
    let mut query = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .into_boxed();

    if !prefix.is_empty() {
        let pattern = format!("{}%", super::escape_like(prefix));
        query = query.filter(objects::key.like(pattern));
    }

    if let Some(marker) = start_after {
        if !marker.is_empty() {
            query = query.filter(objects::key.gt(marker));
        }
    }

    let rows: Vec<ObjectRow> = query
        .order(objects::key.asc())
        .limit(limit)
        .select(ObjectRow::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let truncated = rows.len() > max_keys;
    let page_rows: Vec<ObjectRow> = rows.into_iter().take(max_keys).collect();
    let next_key = if truncated {
        page_rows.last().map(|r| r.key.clone())
    } else {
        None
    };

    let mut metas = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        metas.push(row_into_meta(&mut conn, row).await?);
    }

    Ok((metas, truncated, next_key))
}
