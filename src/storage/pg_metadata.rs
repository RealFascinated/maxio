use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::config::MemoryCacheLimits;
use crate::db::repos::{self, PutBucketContext};
use crate::db::{DbContext, DbPool};
use crate::metrics::MetricsRegistry;

use super::metadata::MetadataStore;
use super::traits::ListPage;
use super::{
    BucketMeta, CorsRule, LifecycleRule, MultipartUploadMeta, ObjectMeta, PartMeta, StorageError,
};

pub struct PgMetadataStore {
    ctx: DbContext,
    metrics: Option<Arc<MetricsRegistry>>,
    limits: MemoryCacheLimits,
}

impl PgMetadataStore {
    pub fn new(pool: Arc<DbPool>, limits: MemoryCacheLimits) -> Self {
        Self {
            ctx: DbContext::new(pool, None, limits),
            metrics: None,
            limits,
        }
    }

    pub fn with_metrics(mut self, metrics: Arc<MetricsRegistry>) -> Self {
        self.metrics = Some(Arc::clone(&metrics));
        self.ctx = DbContext::new(self.ctx.pool_arc(), Some(metrics), self.limits);
        self
    }

    #[inline]
    fn record(&self, op: &str, elapsed: Duration) {
        if let Some(ref m) = self.metrics {
            m.record_metadata_op(op, elapsed);
        }
    }
}

macro_rules! meta_op {
    ($self:expr, $op:literal, $body:expr) => {{
        let __meta_t = Instant::now();
        let __meta_r = $body;
        $self.record($op, __meta_t.elapsed());
        __meta_r
    }};
}

#[async_trait]
impl MetadataStore for PgMetadataStore {
    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError> {
        meta_op!(
            self,
            "create_bucket",
            repos::create_bucket(&self.ctx, meta).await
        )
    }

    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError> {
        meta_op!(
            self,
            "head_bucket",
            repos::head_bucket(&self.ctx, name).await
        )
    }

    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError> {
        meta_op!(
            self,
            "delete_bucket",
            repos::delete_bucket(&self.ctx, name).await
        )
    }

    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError> {
        meta_op!(self, "list_buckets", repos::list_buckets(&self.ctx).await)
    }

    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError> {
        meta_op!(
            self,
            "put_bucket_policy",
            repos::put_bucket_policy(&self.ctx, bucket, policy).await
        )
    }

    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError> {
        meta_op!(
            self,
            "get_bucket_policy",
            repos::get_bucket_policy(&self.ctx, bucket).await
        )
    }

    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError> {
        meta_op!(
            self,
            "delete_bucket_policy",
            repos::delete_bucket_policy(&self.ctx, bucket).await
        )
    }

    async fn put_bucket_acl(&self, bucket: &str, acl: crate::iam::Acl) -> Result<(), StorageError> {
        meta_op!(
            self,
            "put_bucket_acl",
            repos::put_bucket_acl(&self.ctx, bucket, acl).await
        )
    }

    async fn get_bucket_acl(&self, bucket: &str) -> Result<crate::iam::Acl, StorageError> {
        meta_op!(
            self,
            "get_bucket_acl",
            repos::get_bucket_acl(&self.ctx, bucket).await
        )
    }

    async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<CorsRule>,
    ) -> Result<(), StorageError> {
        meta_op!(
            self,
            "put_bucket_cors",
            repos::put_bucket_cors(&self.ctx, bucket, rules).await
        )
    }

    async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>, StorageError> {
        meta_op!(self, "get_bucket_cors", {
            Ok(repos::get_bucket_cors(&self.ctx, bucket)
                .await?
                .unwrap_or_default())
        })
    }

    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError> {
        meta_op!(
            self,
            "delete_bucket_cors",
            repos::delete_bucket_cors(&self.ctx, bucket).await
        )
    }

    async fn put_bucket_lifecycle(
        &self,
        bucket: &str,
        rules: Vec<LifecycleRule>,
    ) -> Result<(), StorageError> {
        meta_op!(
            self,
            "put_bucket_lifecycle",
            repos::put_bucket_lifecycle(&self.ctx, bucket, &rules).await
        )
    }

    async fn get_bucket_lifecycle(&self, bucket: &str) -> Result<Vec<LifecycleRule>, StorageError> {
        meta_op!(
            self,
            "get_bucket_lifecycle",
            repos::get_bucket_lifecycle(&self.ctx, bucket).await
        )
    }

    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<(), StorageError> {
        meta_op!(
            self,
            "delete_bucket_lifecycle",
            repos::delete_bucket_lifecycle(&self.ctx, bucket).await
        )
    }

    async fn list_buckets_with_lifecycle(
        &self,
    ) -> Result<Vec<repos::BucketLifecycleEntry>, StorageError> {
        meta_op!(
            self,
            "list_buckets_with_lifecycle",
            repos::list_buckets_with_lifecycle(&self.ctx).await
        )
    }

    async fn list_expired_current_objects(
        &self,
        bucket_id: uuid::Uuid,
        prefix: &str,
        cutoff: chrono::DateTime<chrono::Utc>,
        limit: i64,
    ) -> Result<Vec<String>, StorageError> {
        meta_op!(
            self,
            "list_expired_current_objects",
            repos::list_expired_current_objects(&self.ctx, bucket_id, prefix, cutoff, limit).await
        )
    }

    async fn list_expired_noncurrent_versions(
        &self,
        bucket_id: uuid::Uuid,
        prefix: &str,
        cutoff: chrono::DateTime<chrono::Utc>,
        limit: i64,
    ) -> Result<Vec<repos::ExpiredVersionRef>, StorageError> {
        meta_op!(
            self,
            "list_expired_noncurrent_versions",
            repos::list_expired_noncurrent_versions(&self.ctx, bucket_id, prefix, cutoff, limit)
                .await
        )
    }

    async fn get_versioning_state(
        &self,
        bucket: &str,
    ) -> Result<crate::storage::VersioningState, StorageError> {
        meta_op!(
            self,
            "get_versioning_state",
            repos::get_versioning_state(&self.ctx, bucket).await
        )
    }

    async fn set_versioning_state(
        &self,
        bucket: &str,
        state: crate::storage::VersioningState,
    ) -> Result<(), StorageError> {
        meta_op!(
            self,
            "set_versioning_state",
            repos::set_versioning_state(&self.ctx, bucket, state).await
        )
    }

    async fn fetch_put_bucket_context(
        &self,
        bucket: &str,
    ) -> Result<PutBucketContext, StorageError> {
        meta_op!(
            self,
            "fetch_put_bucket_context",
            repos::fetch_put_bucket_context(&self.ctx, bucket).await
        )
    }

    async fn fetch_bucket_auth_context(
        &self,
        bucket: &str,
    ) -> Result<repos::BucketAuthSnapshot, StorageError> {
        meta_op!(
            self,
            "fetch_bucket_auth_context",
            repos::fetch_bucket_auth_context(&self.ctx, bucket).await
        )
    }

    async fn upsert_object(
        &self,
        bucket: &str,
        meta: &ObjectMeta,
        put_ctx: Option<&PutBucketContext>,
    ) -> Result<(), StorageError> {
        meta_op!(self, "upsert_object", {
            repos::upsert_object(&self.ctx, bucket, meta, put_ctx).await?;
            Ok(())
        })
    }

    fn defer_object_upsert(
        &self,
        bucket: &str,
        meta: &ObjectMeta,
        put_ctx: Option<&PutBucketContext>,
    ) {
        repos::defer_object_upsert(&self.ctx, bucket, meta, put_ctx.cloned());
    }

    async fn get_object_meta(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        meta_op!(
            self,
            "get_object_meta",
            repos::get_object_meta(&self.ctx, bucket, key).await
        )
    }

    async fn get_object_for_read(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMeta, StorageError> {
        meta_op!(
            self,
            "get_object_for_read",
            repos::get_object_for_read(&self.ctx, bucket, key).await
        )
    }

    async fn delete_object_meta(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        meta_op!(
            self,
            "delete_object_meta",
            repos::delete_object(&self.ctx, bucket, key).await
        )
    }

    async fn delete_objects_by_keys(
        &self,
        bucket: &str,
        keys: &[String],
    ) -> Result<Vec<String>, StorageError> {
        meta_op!(
            self,
            "delete_objects_by_keys",
            repos::delete_objects_by_keys(&self.ctx, bucket, keys).await
        )
    }

    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: Option<&str>,
        max_keys: usize,
        search: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        meta_op!(self, "list_objects_page", {
            let (objects, is_truncated, next) = repos::list_objects_page(
                &self.ctx,
                bucket,
                prefix,
                start_after,
                max_keys,
                search,
                repos::SortOrder::Asc,
            )
            .await?;
            Ok(ListPage {
                objects,
                is_truncated,
                next_continuation: next,
            })
        })
    }

    async fn list_objects_delimited_page(
        &self,
        bucket: &str,
        prefix: &str,
        delimiter: &str,
        start_after: Option<&str>,
        max_keys: usize,
        search: Option<&str>,
        sort: repos::ConsoleListSort,
        order: repos::SortOrder,
    ) -> Result<crate::storage::traits::DelimitedListPage, StorageError> {
        meta_op!(self, "list_objects_delimited_page", {
            repos::list_objects_delimited_page(
                &self.ctx,
                bucket,
                prefix,
                delimiter,
                start_after,
                max_keys,
                search,
                sort,
                order,
            )
            .await
        })
    }

    async fn put_object_acl(
        &self,
        bucket: &str,
        key: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError> {
        meta_op!(
            self,
            "put_object_acl",
            repos::put_object_acl(&self.ctx, bucket, key, acl).await
        )
    }

    async fn get_object_acl(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<crate::iam::Acl, StorageError> {
        meta_op!(
            self,
            "get_object_acl",
            repos::get_object_acl(&self.ctx, bucket, key).await
        )
    }

    async fn put_object_tags(
        &self,
        bucket: &str,
        key: &str,
        tags: HashMap<String, String>,
    ) -> Result<(), StorageError> {
        meta_op!(
            self,
            "put_object_tags",
            repos::put_object_tags(&self.ctx, bucket, key, tags).await
        )
    }

    async fn get_object_tags(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<HashMap<String, String>, StorageError> {
        meta_op!(
            self,
            "get_object_tags",
            repos::get_object_tags(&self.ctx, bucket, key).await
        )
    }

    async fn delete_object_tags(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        meta_op!(
            self,
            "delete_object_tags",
            repos::delete_object_tags(&self.ctx, bucket, key).await
        )
    }

    async fn insert_version(&self, bucket: &str, meta: &ObjectMeta) -> Result<(), StorageError> {
        meta_op!(self, "insert_version", {
            repos::insert_version(&self.ctx, bucket, meta, true).await?;
            Ok(())
        })
    }

    async fn get_object_version_meta(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        meta_op!(
            self,
            "get_object_version_meta",
            repos::get_object_version_meta(&self.ctx, bucket, key, version_id).await
        )
    }

    async fn delete_object_version_meta(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        meta_op!(self, "delete_object_version_meta", {
            repos::delete_object_version(&self.ctx, bucket, key, version_id).await?;
            Ok(())
        })
    }

    async fn delete_object_versions_batch(
        &self,
        bucket: &str,
        pairs: &[(String, String)],
    ) -> Result<Vec<(String, String, bool)>, StorageError> {
        meta_op!(
            self,
            "delete_object_versions_batch",
            repos::delete_object_versions_batch(&self.ctx, bucket, pairs).await
        )
    }

    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        meta_op!(
            self,
            "list_object_versions",
            repos::list_object_versions(&self.ctx, bucket, prefix).await
        )
    }

    async fn list_object_versions_page(
        &self,
        bucket: &str,
        prefix: &str,
        key_marker: Option<&str>,
        version_id_marker: Option<&str>,
        max_keys: usize,
    ) -> Result<repos::VersionsPage, StorageError> {
        meta_op!(
            self,
            "list_object_versions_page",
            repos::list_object_versions_page(
                &self.ctx,
                bucket,
                prefix,
                key_marker,
                version_id_marker,
                max_keys,
            )
            .await
        )
    }

    async fn update_current_after_delete(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(), StorageError> {
        meta_op!(
            self,
            "update_current_after_delete",
            repos::update_current_after_delete(&self.ctx, bucket, key).await
        )
    }

    async fn create_multipart_upload(
        &self,
        meta: &MultipartUploadMeta,
    ) -> Result<(), StorageError> {
        meta_op!(
            self,
            "create_multipart_upload",
            repos::insert_multipart_upload(&self.ctx, meta).await
        )
    }

    async fn get_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<MultipartUploadMeta, StorageError> {
        meta_op!(
            self,
            "get_multipart_upload",
            repos::get_multipart_upload(&self.ctx, upload_id).await
        )
    }

    async fn load_multipart_session(
        &self,
        upload_id: &str,
    ) -> Result<(MultipartUploadMeta, Vec<PartMeta>), StorageError> {
        meta_op!(
            self,
            "load_multipart_session",
            repos::load_multipart_session(&self.ctx, upload_id).await
        )
    }

    async fn abort_multipart_upload(&self, upload_id: &str) -> Result<(), StorageError> {
        meta_op!(
            self,
            "abort_multipart_upload",
            repos::abort_multipart_upload(&self.ctx, upload_id).await
        )
    }

    async fn upsert_part(&self, upload_id: &str, part: &PartMeta) -> Result<(), StorageError> {
        meta_op!(
            self,
            "upsert_part",
            repos::upsert_part(&self.ctx, upload_id, part).await
        )
    }

    async fn list_parts(&self, upload_id: &str) -> Result<Vec<PartMeta>, StorageError> {
        meta_op!(
            self,
            "list_parts",
            repos::list_parts(&self.ctx, upload_id).await
        )
    }

    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError> {
        meta_op!(self, "list_multipart_uploads", {
            let mut uploads = repos::list_multipart_uploads(&self.ctx, bucket).await?;
            if let Some(prefix) = prefix {
                uploads.retain(|u| u.key.starts_with(prefix));
            }
            Ok(uploads)
        })
    }

    async fn cleanup_stale_uploads(
        &self,
        stale_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StorageError> {
        meta_op!(self, "cleanup_stale_uploads", {
            let stale_after = chrono::Utc::now().signed_duration_since(stale_before);
            repos::cleanup_stale_uploads(&self.ctx, stale_after).await
        })
    }
}
