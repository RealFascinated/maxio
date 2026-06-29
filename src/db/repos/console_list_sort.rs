use crate::db::DbContext;
use crate::db::schema::objects;
use crate::storage::traits::DelimitedListPage;
use crate::storage::{ObjectMeta, StorageError};
use chrono::{DateTime, Utc};
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::Bool;
use diesel_async::RunQueryDsl;

use super::listing::{DELIMITED_SCAN_BATCH, delimited_common_prefix, list_objects_page};
use super::objects::{ObjectRow, row_into_read_meta};
use super::{db_err, get_conn, resolve_bucket_id};

const FILE_CURSOR_PREFIX: &str = "file\x1f";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConsoleListSort {
    #[default]
    Name,
    Size,
    Modified,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl ConsoleListSort {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("size") => Self::Size,
            Some("modified") => Self::Modified,
            Some("type") => Self::Type,
            _ => Self::Name,
        }
    }
}

impl SortOrder {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("desc") => Self::Desc,
            _ => Self::Asc,
        }
    }
}

pub fn is_file_cursor(start_after: Option<&str>) -> bool {
    start_after.is_some_and(|cursor| cursor.starts_with(FILE_CURSOR_PREFIX))
}

/// Folders-first console listing with SQL-backed file sorting.
pub async fn list_objects_delimited_page_composed(
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
    let prefix_order = if sort == ConsoleListSort::Name {
        order
    } else {
        SortOrder::Asc
    };

    let first_page = !is_file_cursor(start_after);
    let prefixes = if first_page {
        list_delimited_prefixes(ctx, bucket_name, prefix, delimiter, search, prefix_order).await?
    } else {
        Vec::new()
    };

    let file_slots = max_keys.saturating_sub(prefixes.len());
    let file_start = encode_file_start_cursor();
    let file_cursor = if is_file_cursor(start_after) {
        start_after
    } else if first_page && file_slots == 0 {
        Some(file_start.as_str())
    } else {
        None
    };

    let file_limit = if is_file_cursor(start_after) {
        max_keys
    } else {
        file_slots.max(1)
    };

    let (files, mut next_continuation) = if file_slots > 0 || is_file_cursor(start_after) {
        list_delimited_direct_files_page(
            ctx,
            bucket_name,
            prefix,
            delimiter,
            sort,
            order,
            file_cursor,
            file_limit,
            search,
        )
        .await?
    } else {
        (Vec::new(), None)
    };

    if next_continuation.is_none() {
        let probe = files
            .last()
            .map(|meta| encode_file_cursor(sort, meta))
            .unwrap_or_else(encode_file_start_cursor);
        if has_more_direct_files(
            ctx,
            bucket_name,
            prefix,
            delimiter,
            sort,
            order,
            Some(&probe),
            search,
        )
        .await?
        {
            next_continuation = Some(probe);
        }
    }

    Ok(DelimitedListPage {
        files,
        prefixes,
        next_continuation,
    })
}

async fn list_delimited_prefixes(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    delimiter: &str,
    search: Option<&str>,
    order: SortOrder,
) -> Result<Vec<String>, StorageError> {
    use std::collections::BTreeSet;

    let mut prefix_set = BTreeSet::new();
    let mut cursor = None;

    loop {
        let (batch, truncated, next) = list_objects_page(
            ctx,
            bucket_name,
            prefix,
            cursor.as_deref(),
            DELIMITED_SCAN_BATCH,
            search,
            SortOrder::Asc,
        )
        .await?;

        if batch.is_empty() {
            break;
        }

        for obj in &batch {
            if let Some(common) = delimited_common_prefix(&obj.key, prefix, delimiter) {
                prefix_set.insert(common);
            }
        }

        if !truncated {
            break;
        }
        cursor = next;
    }

    let mut prefixes: Vec<String> = prefix_set.into_iter().collect();
    if order == SortOrder::Desc {
        prefixes.reverse();
    }
    Ok(prefixes)
}

async fn list_delimited_direct_files_page(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    delimiter: &str,
    sort: ConsoleListSort,
    order: SortOrder,
    start_after: Option<&str>,
    max_keys: usize,
    search: Option<&str>,
) -> Result<(Vec<ObjectMeta>, Option<String>), StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;
    let bucket_id = resolve_bucket_id(ctx.bucket_cache(), &mut conn, bucket_name).await?;

    let limit = max_keys.saturating_add(1) as i64;
    let mut query = objects::table
        .filter(objects::bucket_id.eq(bucket_id))
        .into_boxed();
    query = apply_direct_child_filter(query, prefix, delimiter);

    if let Some(q) = search.filter(|s| !s.is_empty()) {
        let pattern = format!("%{}%", super::escape_like(q));
        query = query.filter(objects::key.ilike(pattern));
    }

    if let Some(cursor) = start_after
        .filter(|c| is_file_cursor(Some(c)))
        .and_then(parse_file_cursor)
    {
        if !matches!(cursor, FileCursor::Start) {
            query = apply_file_cursor_filter(query, sort, order, cursor);
        }
    }

    query = apply_file_sort_order(query, sort, order);

    let rows: Vec<ObjectRow> = query
        .limit(limit)
        .select(ObjectRow::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let truncated = rows.len() > max_keys;
    let metas: Vec<ObjectMeta> = rows
        .into_iter()
        .take(max_keys)
        .map(row_into_read_meta)
        .collect();
    let next = truncated
        .then(|| metas.last().map(|meta| encode_file_cursor(sort, meta)))
        .flatten();

    Ok((metas, next))
}

async fn has_more_direct_files(
    ctx: &DbContext,
    bucket_name: &str,
    prefix: &str,
    delimiter: &str,
    sort: ConsoleListSort,
    order: SortOrder,
    after_cursor: Option<&str>,
    search: Option<&str>,
) -> Result<bool, StorageError> {
    let (files, _) = list_delimited_direct_files_page(
        ctx,
        bucket_name,
        prefix,
        delimiter,
        sort,
        order,
        after_cursor,
        1,
        search,
    )
    .await?;
    Ok(!files.is_empty())
}

fn apply_direct_child_filter<'a>(
    mut query: diesel::dsl::IntoBoxed<'a, objects::table, diesel::pg::Pg>,
    prefix: &str,
    delimiter: &str,
) -> diesel::dsl::IntoBoxed<'a, objects::table, diesel::pg::Pg> {
    if !prefix.is_empty() {
        let pattern = format!("{}%", super::escape_like(prefix));
        query = query.filter(objects::key.like(pattern));
    }
    query = query.filter(objects::key.not_like("%/"));
    let from = (prefix.len() + 1) as i32;
    let delim = delimiter.replace('\'', "''");
    query.filter(sql::<Bool>(&format!(
        "strpos(substring(key from {from}), '{delim}') = 0"
    )))
}

#[derive(Debug, Clone)]
enum FileCursor {
    Start,
    Name { key: String },
    Size { size: i64, key: String },
    Modified { at: DateTime<Utc>, key: String },
    Type { content_type: String, key: String },
}

fn encode_file_start_cursor() -> String {
    format!("{FILE_CURSOR_PREFIX}start")
}

fn encode_file_cursor(sort: ConsoleListSort, meta: &ObjectMeta) -> String {
    match sort {
        ConsoleListSort::Name => format!("{FILE_CURSOR_PREFIX}name\x1f{}", meta.key),
        ConsoleListSort::Size => {
            format!("{FILE_CURSOR_PREFIX}size\x1f{}\x1f{}", meta.size, meta.key)
        }
        ConsoleListSort::Modified => {
            let micros = parse_modified_micros(&meta.last_modified).unwrap_or(0);
            format!("{FILE_CURSOR_PREFIX}modified\x1f{micros}\x1f{}", meta.key)
        }
        ConsoleListSort::Type => format!(
            "{FILE_CURSOR_PREFIX}type\x1f{}\x1f{}",
            meta.content_type, meta.key
        ),
    }
}

fn parse_file_cursor(cursor: &str) -> Option<FileCursor> {
    let rest = cursor.strip_prefix(FILE_CURSOR_PREFIX)?;
    if rest == "start" {
        return Some(FileCursor::Start);
    }

    let (kind, payload) = rest.split_once('\x1f')?;
    match kind {
        "name" => Some(FileCursor::Name {
            key: payload.to_string(),
        }),
        "size" => {
            let (size, key) = payload.split_once('\x1f')?;
            Some(FileCursor::Size {
                size: size.parse().ok()?,
                key: key.to_string(),
            })
        }
        "modified" => {
            let (micros, key) = payload.split_once('\x1f')?;
            Some(FileCursor::Modified {
                at: DateTime::from_timestamp_micros(micros.parse().ok()?).unwrap_or_default(),
                key: key.to_string(),
            })
        }
        "type" => {
            let (content_type, key) = payload.split_once('\x1f')?;
            Some(FileCursor::Type {
                content_type: content_type.to_string(),
                key: key.to_string(),
            })
        }
        _ => None,
    }
}

fn parse_modified_micros(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_micros())
}

fn apply_file_sort_order<'a>(
    query: diesel::dsl::IntoBoxed<'a, objects::table, diesel::pg::Pg>,
    sort: ConsoleListSort,
    order: SortOrder,
) -> diesel::dsl::IntoBoxed<'a, objects::table, diesel::pg::Pg> {
    use SortOrder::{Asc, Desc};
    match (sort, order) {
        (ConsoleListSort::Name, Asc) => query.order(objects::key.asc()),
        (ConsoleListSort::Name, Desc) => query.order(objects::key.desc()),
        (ConsoleListSort::Size, Asc) => query.order((objects::size.asc(), objects::key.asc())),
        (ConsoleListSort::Size, Desc) => query.order((objects::size.desc(), objects::key.asc())),
        (ConsoleListSort::Modified, Asc) => {
            query.order((objects::last_modified.asc(), objects::key.asc()))
        }
        (ConsoleListSort::Modified, Desc) => {
            query.order((objects::last_modified.desc(), objects::key.asc()))
        }
        (ConsoleListSort::Type, Asc) => {
            query.order((objects::content_type.asc(), objects::key.asc()))
        }
        (ConsoleListSort::Type, Desc) => {
            query.order((objects::content_type.desc(), objects::key.asc()))
        }
    }
}

fn apply_file_cursor_filter<'a>(
    query: diesel::dsl::IntoBoxed<'a, objects::table, diesel::pg::Pg>,
    sort: ConsoleListSort,
    order: SortOrder,
    cursor: FileCursor,
) -> diesel::dsl::IntoBoxed<'a, objects::table, diesel::pg::Pg> {
    macro_rules! after_value {
        (asc, $query:expr, $column:expr, $value:expr, $key:expr) => {
            $query.filter(
                $column
                    .gt($value)
                    .or($column.eq($value).and(objects::key.gt($key))),
            )
        };
        (desc, $query:expr, $column:expr, $value:expr, $key:expr) => {
            $query.filter(
                $column
                    .lt($value)
                    .or($column.eq($value).and(objects::key.gt($key))),
            )
        };
    }

    match (sort, order, cursor) {
        (ConsoleListSort::Name, SortOrder::Asc, FileCursor::Name { key }) => {
            query.filter(objects::key.gt(key))
        }
        (ConsoleListSort::Name, SortOrder::Desc, FileCursor::Name { key }) => {
            query.filter(objects::key.lt(key))
        }
        (ConsoleListSort::Size, SortOrder::Asc, FileCursor::Size { size, key }) => {
            after_value!(asc, query, objects::size, size, key)
        }
        (ConsoleListSort::Size, SortOrder::Desc, FileCursor::Size { size, key }) => {
            after_value!(desc, query, objects::size, size, key)
        }
        (ConsoleListSort::Modified, SortOrder::Asc, FileCursor::Modified { at, key }) => {
            after_value!(asc, query, objects::last_modified, at, key)
        }
        (ConsoleListSort::Modified, SortOrder::Desc, FileCursor::Modified { at, key }) => {
            after_value!(desc, query, objects::last_modified, at, key)
        }
        (ConsoleListSort::Type, SortOrder::Asc, FileCursor::Type { content_type, key }) => {
            after_value!(asc, query, objects::content_type, content_type.clone(), key)
        }
        (ConsoleListSort::Type, SortOrder::Desc, FileCursor::Type { content_type, key }) => {
            after_value!(
                desc,
                query,
                objects::content_type,
                content_type.clone(),
                key
            )
        }
        _ => query,
    }
}
