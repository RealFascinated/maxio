use super::console_list_sort;

use std::collections::{BTreeSet, HashMap};

use crate::db::DbContext;
use crate::db::schema::objects;
use crate::storage::traits::DelimitedListPage;
use crate::storage::{ObjectMeta, StorageError};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use super::objects::{ObjectRow, row_into_read_meta};
use super::{db_err, get_conn, resolve_bucket_id};

pub use console_list_sort::{ConsoleListSort, SortOrder, is_file_cursor};

pub(crate) const DELIMITED_SCAN_BATCH: usize = 200;

/// True when `key` is a direct file at `prefix` with `delimiter` (not a folder marker or nested path).
pub fn delimited_direct_file(key: &str, prefix: &str, delimiter: &str) -> bool {
    if !key.starts_with(prefix) {
        return false;
    }
    let suffix = &key[prefix.len()..];
    if suffix.is_empty() || key.ends_with('/') {
        return false;
    }
    !suffix.contains(delimiter)
}

/// Common prefix for `key` at `prefix` depth, or `None` when the key is not a folder entry.
pub fn delimited_common_prefix(key: &str, prefix: &str, delimiter: &str) -> Option<String> {
    if !key.starts_with(prefix) {
        return None;
    }
    if key.ends_with('/') {
        if key != prefix {
            return Some(key.to_string());
        }
        return None;
    }
    let suffix = &key[prefix.len()..];
    if let Some(pos) = suffix.find(delimiter) {
        let common = format!("{}{}", prefix, &suffix[..pos + delimiter.len()]);
        if common != prefix {
            return Some(common);
        }
    }
    None
}

pub async fn max_key_under_prefix(
    ctx: &DbContext,
    bucket_name: &str,
    key_prefix: &str,
) -> Result<Option<String>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;
    let pattern = format!("{}%", super::escape_like(key_prefix));

    objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .filter(objects::key.like(pattern))
        .order(objects::key.desc())
        .select(objects::key)
        .first::<String>(&mut conn)
        .await
        .optional()
        .map_err(db_err)
}

pub async fn list_objects_page(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    start_after: Option<&str>,
    max_keys: usize,
    search: Option<&str>,
    order: SortOrder,
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

    if let Some(marker) = start_after.filter(|m| !m.is_empty()) {
        query = match order {
            SortOrder::Asc => query.filter(objects::key.gt(marker)),
            SortOrder::Desc => query.filter(objects::key.lt(marker)),
        };
    }

    if let Some(q) = search.filter(|s| !s.is_empty()) {
        let pattern = format!("%{}%", super::escape_like(q));
        query = query.filter(objects::key.ilike(pattern));
    }

    query = match order {
        SortOrder::Asc => query.order(objects::key.asc()),
        SortOrder::Desc => query.order(objects::key.desc()),
    };

    let rows: Vec<ObjectRow> = query
        .limit(limit)
        .select(ObjectRow::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let truncated = rows.len() > max_keys;
    let page_rows: Vec<ObjectRow> = rows.into_iter().take(max_keys).collect();
    let next_key = truncated
        .then(|| page_rows.last().map(|r| r.key.clone()))
        .flatten();
    let metas = page_rows.into_iter().map(row_into_read_meta).collect();

    Ok((metas, truncated, next_key))
}

/// List objects collapsed by `delimiter`, skipping entire subtrees after each common prefix.
pub async fn list_objects_delimited_page(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    delimiter: &str,
    start_after: Option<&str>,
    max_keys: usize,
    search: Option<&str>,
    sort: ConsoleListSort,
    order: SortOrder,
) -> Result<DelimitedListPage, StorageError> {
    if sort == ConsoleListSort::Name && order == SortOrder::Asc && !is_file_cursor(start_after) {
        return list_objects_delimited_page_key_scan(
            ctx,
            bucket_name,
            prefix,
            delimiter,
            start_after,
            max_keys,
            search,
            SortOrder::Asc,
        )
        .await;
    }

    console_list_sort::list_objects_delimited_page_composed(
        ctx,
        bucket_name,
        prefix,
        delimiter,
        start_after,
        max_keys,
        search,
        sort,
        order,
    )
    .await
}

async fn list_objects_delimited_page_key_scan(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    delimiter: &str,
    start_after: Option<&str>,
    max_keys: usize,
    search: Option<&str>,
    order: SortOrder,
) -> Result<DelimitedListPage, StorageError> {
    let mut files = Vec::new();
    let mut prefix_set = BTreeSet::new();
    let mut prefix_max_keys: HashMap<String, String> = HashMap::new();
    let mut cursor = start_after.map(str::to_string);
    let mut next_continuation = None;

    'outer: loop {
        let (batch, batch_truncated, batch_next) = list_objects_page(
            ctx,
            bucket_name,
            prefix,
            cursor.as_deref(),
            DELIMITED_SCAN_BATCH,
            search,
            order,
        )
        .await?;

        if batch.is_empty() {
            break;
        }

        let mut i = 0;
        while i < batch.len() {
            if files.len() + prefix_set.len() >= max_keys {
                let more_in_batch = i + 1 < batch.len();
                if more_in_batch || batch_truncated {
                    next_continuation = Some(batch[i].key.clone());
                } else if has_keys_after(
                    ctx,
                    bucket_name,
                    prefix,
                    Some(&batch[i].key),
                    search,
                    order,
                )
                .await?
                {
                    next_continuation = Some(batch[i].key.clone());
                }
                break 'outer;
            }

            let obj = &batch[i];
            if delimited_direct_file(&obj.key, prefix, delimiter) {
                files.push(obj.clone());
                i += 1;
                continue;
            }

            if let Some(common) = delimited_common_prefix(&obj.key, prefix, delimiter) {
                prefix_set.insert(common.clone());
                let max_key = match prefix_max_keys.get(&common) {
                    Some(key) => key.clone(),
                    None => {
                        let key = max_key_under_prefix(ctx, bucket_name, &common)
                            .await?
                            .unwrap_or_else(|| obj.key.clone());
                        prefix_max_keys.insert(common.clone(), key.clone());
                        key
                    }
                };
                i += 1;
                while i < batch.len() && batch[i].key <= max_key {
                    i += 1;
                }
                continue;
            }

            i += 1;
        }

        if files.len() + prefix_set.len() >= max_keys {
            if batch_truncated {
                next_continuation = batch.last().map(|o| o.key.clone());
            } else {
                let last_key = batch.last().map(|o| o.key.as_str());
                if has_keys_after(ctx, bucket_name, prefix, last_key, search, order).await? {
                    next_continuation = batch.last().map(|o| o.key.clone());
                }
            }
            break;
        }

        if !batch_truncated {
            break;
        }
        cursor = batch_next;
    }

    Ok(DelimitedListPage {
        files,
        prefixes: prefix_set.into_iter().collect(),
        next_continuation,
    })
}

async fn has_keys_after(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    after: Option<&str>,
    search: Option<&str>,
    order: SortOrder,
) -> Result<bool, StorageError> {
    let (objects, _, _) =
        list_objects_page(ctx, bucket_name, prefix, after, 1, search, order).await?;
    Ok(!objects.is_empty())
}
