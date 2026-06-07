use async_trait::async_trait;
use std::collections::HashMap;

use super::{
    BucketMeta, CorsRule, MultipartUploadMeta, ObjectMeta, PartMeta,
    StorageError,
};
use super::traits::ListPage;

#[async_trait]
pub trait MetadataStore: Send + Sync {
    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError>;
    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError>;
    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError>;
    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError>;
    async fn get_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError>;
    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError>;
    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError>;
    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError>;
    async fn put_bucket_acl(
        &self,
        bucket: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError>;
    async fn get_bucket_acl(&self, bucket: &str) -> Result<crate::iam::Acl, StorageError>;
    async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<CorsRule>,
    ) -> Result<(), StorageError>;
    async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>, StorageError>;
    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError>;
    async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError>;
    async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError>;

    async fn upsert_object(&self, bucket: &str, meta: &ObjectMeta) -> Result<(), StorageError>;
    async fn get_object_meta(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMeta, StorageError>;
    async fn delete_object_meta(&self, bucket: &str, key: &str) -> Result<(), StorageError>;
    async fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError>;
    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: Option<&str>,
        max_keys: usize,
    ) -> Result<ListPage, StorageError>;
    async fn put_object_acl(
        &self,
        bucket: &str,
        key: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError>;
    async fn get_object_acl(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<crate::iam::Acl, StorageError>;
    async fn put_object_tags(
        &self,
        bucket: &str,
        key: &str,
        tags: HashMap<String, String>,
    ) -> Result<(), StorageError>;
    async fn get_object_tags(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<HashMap<String, String>, StorageError>;
    async fn delete_object_tags(&self, bucket: &str, key: &str) -> Result<(), StorageError>;

    async fn insert_version(&self, bucket: &str, meta: &ObjectMeta) -> Result<(), StorageError>;
    async fn get_object_version_meta(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError>;
    async fn delete_object_version_meta(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError>;
    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError>;
    async fn set_current_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError>;
    async fn update_current_after_delete(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(), StorageError>;

    async fn create_multipart_upload(
        &self,
        meta: &MultipartUploadMeta,
    ) -> Result<(), StorageError>;
    async fn get_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<MultipartUploadMeta, StorageError>;
    async fn abort_multipart_upload(&self, upload_id: &str) -> Result<(), StorageError>;
    async fn upsert_part(&self, upload_id: &str, part: &PartMeta) -> Result<(), StorageError>;
    async fn delete_part(
        &self,
        upload_id: &str,
        part_number: u32,
    ) -> Result<(), StorageError>;
    async fn list_parts(
        &self,
        upload_id: &str,
    ) -> Result<Vec<PartMeta>, StorageError>;
    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError>;
    async fn cleanup_stale_uploads(
        &self,
        stale_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StorageError>;
}
