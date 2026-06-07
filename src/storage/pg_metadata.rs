use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::db::repos;
use crate::db::DbPool;

use super::metadata::MetadataStore;
use super::traits::ListPage;
use super::{
    BucketMeta, CorsRule, MultipartUploadMeta, ObjectMeta, PartMeta, StorageError,
};

pub struct PgMetadataStore {
    pool: Arc<DbPool>,
}

impl PgMetadataStore {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &DbPool {
        &self.pool
    }
}

#[async_trait]
impl MetadataStore for PgMetadataStore {
    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError> {
        repos::create_bucket(self.pool(), meta).await
    }

    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError> {
        repos::head_bucket(self.pool(), name).await
    }

    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError> {
        repos::delete_bucket(self.pool(), name).await
    }

    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError> {
        repos::list_buckets(self.pool()).await
    }

    async fn get_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError> {
        repos::get_bucket_meta(self.pool(), bucket).await
    }

    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError> {
        repos::put_bucket_policy(self.pool(), bucket, policy).await
    }

    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError> {
        repos::get_bucket_policy(self.pool(), bucket).await
    }

    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError> {
        repos::delete_bucket_policy(self.pool(), bucket).await
    }

    async fn put_bucket_acl(
        &self,
        bucket: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError> {
        repos::put_bucket_acl(self.pool(), bucket, acl).await
    }

    async fn get_bucket_acl(&self, bucket: &str) -> Result<crate::iam::Acl, StorageError> {
        repos::get_bucket_acl(self.pool(), bucket).await
    }

    async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<CorsRule>,
    ) -> Result<(), StorageError> {
        repos::put_bucket_cors(self.pool(), bucket, rules).await
    }

    async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>, StorageError> {
        Ok(repos::get_bucket_cors(self.pool(), bucket)
            .await?
            .unwrap_or_default())
    }

    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError> {
        repos::delete_bucket_cors(self.pool(), bucket).await
    }

    async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError> {
        repos::is_versioned(self.pool(), bucket).await
    }

    async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError> {
        repos::set_versioning(self.pool(), bucket, enabled).await
    }

    async fn upsert_object(&self, bucket: &str, meta: &ObjectMeta) -> Result<(), StorageError> {
        repos::upsert_object(self.pool(), bucket, meta).await?;
        Ok(())
    }

    async fn get_object_meta(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMeta, StorageError> {
        repos::get_object_meta(self.pool(), bucket, key).await
    }

    async fn delete_object_meta(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        repos::delete_object(self.pool(), bucket, key).await
    }

    async fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError> {
        repos::object_exists(self.pool(), bucket, key).await
    }

    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: Option<&str>,
        max_keys: usize,
    ) -> Result<ListPage, StorageError> {
        let (objects, is_truncated, next) =
            repos::list_objects_page(self.pool(), bucket, prefix, start_after, max_keys).await?;
        Ok(ListPage {
            objects,
            is_truncated,
            next_continuation: next,
        })
    }

    async fn put_object_acl(
        &self,
        bucket: &str,
        key: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError> {
        repos::put_object_acl(self.pool(), bucket, key, acl).await
    }

    async fn get_object_acl(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<crate::iam::Acl, StorageError> {
        repos::get_object_acl(self.pool(), bucket, key).await
    }

    async fn put_object_tags(
        &self,
        bucket: &str,
        key: &str,
        tags: HashMap<String, String>,
    ) -> Result<(), StorageError> {
        repos::put_object_tags(self.pool(), bucket, key, tags).await
    }

    async fn get_object_tags(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<HashMap<String, String>, StorageError> {
        repos::get_object_tags(self.pool(), bucket, key).await
    }

    async fn delete_object_tags(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        repos::delete_object_tags(self.pool(), bucket, key).await
    }

    async fn insert_version(&self, bucket: &str, meta: &ObjectMeta) -> Result<(), StorageError> {
        repos::insert_version(self.pool(), bucket, meta, true).await?;
        Ok(())
    }

    async fn get_object_version_meta(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        repos::get_object_version_meta(self.pool(), bucket, key, version_id).await
    }

    async fn delete_object_version_meta(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        repos::delete_object_version(self.pool(), bucket, key, version_id).await?;
        Ok(())
    }

    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        repos::list_object_versions(self.pool(), bucket, prefix).await
    }

    async fn set_current_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        repos::set_current_version(self.pool(), bucket, key, version_id).await
    }

    async fn update_current_after_delete(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(), StorageError> {
        repos::update_current_after_delete(self.pool(), bucket, key).await
    }

    async fn create_multipart_upload(
        &self,
        meta: &MultipartUploadMeta,
    ) -> Result<(), StorageError> {
        repos::insert_multipart_upload(self.pool(), meta).await
    }

    async fn get_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<MultipartUploadMeta, StorageError> {
        repos::get_multipart_upload(self.pool(), upload_id).await
    }

    async fn abort_multipart_upload(&self, upload_id: &str) -> Result<(), StorageError> {
        repos::abort_multipart_upload(self.pool(), upload_id).await
    }

    async fn upsert_part(&self, upload_id: &str, part: &PartMeta) -> Result<(), StorageError> {
        repos::upsert_part(self.pool(), upload_id, part).await
    }

    async fn delete_part(
        &self,
        upload_id: &str,
        part_number: u32,
    ) -> Result<(), StorageError> {
        repos::delete_part(self.pool(), upload_id, part_number).await
    }

    async fn list_parts(&self, upload_id: &str) -> Result<Vec<PartMeta>, StorageError> {
        let (_, parts) = repos::list_parts(self.pool(), upload_id).await?;
        Ok(parts)
    }

    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError> {
        let mut uploads = repos::list_multipart_uploads(self.pool(), bucket).await?;
        if let Some(prefix) = prefix {
            uploads.retain(|u| u.key.starts_with(prefix));
        }
        Ok(uploads)
    }

    async fn cleanup_stale_uploads(
        &self,
        stale_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StorageError> {
        let stale_after = chrono::Utc::now().signed_duration_since(stale_before);
        repos::cleanup_stale_uploads(self.pool(), stale_after).await
    }
}
