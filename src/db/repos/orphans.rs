use crate::db::DbContext;
use crate::db::schema::{buckets, object_versions, objects};
use crate::storage::StorageError;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use super::{db_err, get_conn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaBlobSource {
    Current,
    Version(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaBlobRef {
    pub bucket: String,
    pub key: String,
    pub source: MetaBlobSource,
}

pub async fn list_blob_backed_meta(ctx: &DbContext) -> Result<Vec<MetaBlobRef>, StorageError> {
    let mut conn = get_conn(ctx.pool()).await?;

    let current_rows: Vec<(String, String)> = objects::table
        .inner_join(buckets::table)
        .filter(objects::is_delete_marker.eq(false))
        .select((buckets::name, objects::key))
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let version_rows: Vec<(String, String, String)> = object_versions::table
        .inner_join(buckets::table)
        .filter(object_versions::is_delete_marker.eq(false))
        .select((
            buckets::name,
            object_versions::key,
            object_versions::version_id,
        ))
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let mut refs = Vec::with_capacity(current_rows.len() + version_rows.len());
    refs.extend(current_rows.into_iter().map(|(bucket, key)| MetaBlobRef {
        bucket,
        key,
        source: MetaBlobSource::Current,
    }));
    refs.extend(
        version_rows
            .into_iter()
            .map(|(bucket, key, version_id)| MetaBlobRef {
                bucket,
                key,
                source: MetaBlobSource::Version(version_id),
            }),
    );
    Ok(refs)
}
