use async_trait::async_trait;
use std::collections::HashMap;

use super::{
    BatchDeleteObject, BucketMeta, ByteStream, ChecksumAlgorithm, CorsRule, DeleteResult,
    MultipartUploadMeta, ObjectMeta, PartMeta, PutResult, StorageError,
};

#[derive(Debug, Clone)]
pub struct ListPage {
    pub objects: Vec<ObjectMeta>,
    pub is_truncated: bool,
    pub next_continuation: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DelimitedListPage {
    pub files: Vec<ObjectMeta>,
    pub prefixes: Vec<String>,
    pub next_continuation: Option<String>,
}

#[async_trait]
pub trait Storage: Send + Sync {
    // Buckets
    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError>;
    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError>;
    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError>;
    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError>;
    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError>;
    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError>;
    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError>;
    async fn put_bucket_acl(&self, bucket: &str, acl: crate::iam::Acl) -> Result<(), StorageError>;
    async fn get_bucket_acl(&self, bucket: &str) -> Result<crate::iam::Acl, StorageError>;
    async fn put_bucket_cors(&self, bucket: &str, rules: Vec<CorsRule>)
    -> Result<(), StorageError>;
    async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>, StorageError>;
    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError>;
    async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError>;
    async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError>;
    async fn get_bucket_auth_info(
        &self,
        bucket: &str,
    ) -> Result<(Option<String>, crate::iam::Acl), StorageError>;
    async fn fetch_bucket_auth_context(
        &self,
        bucket: &str,
    ) -> Result<crate::db::repos::BucketAuthSnapshot, StorageError>;

    // Objects
    async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PutResult, StorageError>;
    async fn get_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(ByteStream, ObjectMeta), StorageError>;
    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        length: u64,
    ) -> Result<(ByteStream, ObjectMeta), StorageError>;
    async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError>;
    /// Open a byte-range stream using a pre-fetched `ObjectMeta`, skipping the DB lookup
    /// that `get_object_range` would perform. Use when the caller already holds the metadata.
    async fn open_range(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
        offset: u64,
        length: u64,
    ) -> Result<ByteStream, StorageError>;
    async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<HashMap<String, String>, StorageError>;
    async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: HashMap<String, String>,
    ) -> Result<(), StorageError>;
    async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), StorageError>;
    async fn delete_object(&self, bucket: &str, key: &str) -> Result<DeleteResult, StorageError>;
    async fn delete_objects_batch(
        &self,
        bucket: &str,
        objects: &[BatchDeleteObject],
    ) -> Result<Vec<(BatchDeleteObject, Result<DeleteResult, StorageError>)>, StorageError>;
    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: Option<&str>,
        max_keys: usize,
        search: Option<&str>,
    ) -> Result<ListPage, StorageError>;
    async fn list_objects_delimited_page(
        &self,
        bucket: &str,
        prefix: &str,
        delimiter: &str,
        start_after: Option<&str>,
        max_keys: usize,
        search: Option<&str>,
    ) -> Result<DelimitedListPage, StorageError>;
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

    // Multipart
    async fn get_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<MultipartUploadMeta, StorageError>;
    async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        checksum_algorithm: Option<ChecksumAlgorithm>,
    ) -> Result<MultipartUploadMeta, StorageError>;
    async fn upload_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PartMeta, StorageError>;
    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<PutResult, StorageError>;
    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError>;
    async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number_marker: Option<u32>,
        max_parts: usize,
    ) -> Result<(Vec<PartMeta>, bool), StorageError>;
    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError>;

    // Versioning
    async fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(ByteStream, ObjectMeta), StorageError>;
    async fn head_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError>;
    async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<DeleteResult, StorageError>;
    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError>;
    async fn list_object_versions_page(
        &self,
        bucket: &str,
        prefix: &str,
        key_marker: Option<&str>,
        version_id_marker: Option<&str>,
        max_keys: usize,
    ) -> Result<crate::db::repos::VersionsPage, StorageError>;

    // Housekeeping
    async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64);
}
