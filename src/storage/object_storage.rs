use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;

use super::blob::{BlobStorage, validate_key, validate_upload_id};
use super::metadata::{MetadataStore, PutBucketContext};
use super::traits::{ListPage, Storage};
use super::{
    BatchDeleteObject, BucketMeta, ByteStream, ChecksumAlgorithm, CorsRule, DeleteResult,
    MultipartUploadMeta, ObjectMeta, PartMeta, PutResult, StorageError, normalize_object_meta,
    validate_bucket_name,
};

const DELETE_BLOB_CONCURRENCY: usize = 32;
use crate::metrics::MetricsRegistry;

pub struct ObjectStorage {
    blobs: BlobStorage,
    meta: Arc<dyn MetadataStore>,
    metrics: Option<Arc<MetricsRegistry>>,
    async_meta_write: bool,
}

impl ObjectStorage {
    pub fn new(blobs: BlobStorage, meta: Arc<dyn MetadataStore>) -> Self {
        Self {
            blobs,
            meta,
            metrics: None,
            async_meta_write: false,
        }
    }

    pub fn with_metrics(mut self, metrics: Arc<MetricsRegistry>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn with_async_meta_write(mut self) -> Self {
        self.async_meta_write = true;
        self
    }

    #[inline]
    fn record(&self, op: &str, elapsed: std::time::Duration, bytes: u64) {
        if let Some(ref m) = self.metrics {
            m.record_storage_op(op, elapsed, bytes);
        }
    }

    async fn bucket_owner(&self, bucket: &str) -> Result<(String, String), StorageError> {
        let prep = self.meta.fetch_put_bucket_context(bucket).await?;
        Ok((prep.owner_id, prep.owner_display_name))
    }

    fn now_ts() -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    }

    async fn finalize_written_object(
        &self,
        bucket: &str,
        key: &str,
        mut object_meta: ObjectMeta,
        written: super::blob::WrittenPayload,
        versioned: bool,
        put_ctx: Option<&PutBucketContext>,
    ) -> Result<PutResult, StorageError> {
        // Fast path: rename bytes into place first, then commit metadata asynchronously.
        // Only safe when versioning is off — versioned puts need the DB write to be
        // synchronous so that insert_version + archive_version are ordered correctly.
        if self.async_meta_write && !versioned {
            BlobStorage::publish_temp_payload(&written.tmp_path, &written.final_path).await?;
            if let Some(ref m) = self.metrics {
                m.record_drive_write_op();
            }

            let result = PutResult {
                size: written.size,
                etag: object_meta.etag.clone(),
                last_modified: object_meta.last_modified.clone(),
                version_id: object_meta.version_id.take(),
                checksum_algorithm: written.checksum_algorithm,
                checksum_value: written.checksum_value,
            };

            self.meta.defer_object_upsert(bucket, &object_meta, put_ctx);

            self.blobs
                .complete_object_write(bucket, key, &written.final_path, written.size)
                .await?;
            return Ok(result);
        }

        // Sync path: commit metadata first, then rename bytes into place.
        self.meta
            .upsert_object(bucket, &object_meta, put_ctx)
            .await?;
        if let Err(e) =
            BlobStorage::publish_temp_payload(&written.tmp_path, &written.final_path).await
        {
            let _ = self.meta.delete_object_meta(bucket, key).await;
            return Err(e);
        }
        if let Some(ref m) = self.metrics {
            m.record_drive_write_op();
        }

        if versioned {
            self.meta.insert_version(bucket, &object_meta).await?;
            self.blobs
                .archive_version(
                    bucket,
                    key,
                    object_meta.version_id.as_ref().unwrap(),
                    &written.final_path,
                )
                .await?;
        }

        self.blobs
            .complete_object_write(bucket, key, &written.final_path, written.size)
            .await?;

        Ok(PutResult {
            size: written.size,
            etag: object_meta.etag.clone(),
            last_modified: object_meta.last_modified.clone(),
            version_id: object_meta.version_id.take(),
            checksum_algorithm: written.checksum_algorithm,
            checksum_value: written.checksum_value,
        })
    }

    async fn sync_current_blobs_after_version_change(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(), StorageError> {
        match self.meta.get_object_meta(bucket, key).await {
            Ok(meta) => {
                if meta.is_delete_marker {
                    self.blobs.unlink_object(bucket, key).await?;
                } else if let Some(ref version_id) = meta.version_id {
                    self.blobs
                        .restore_current_from_version(bucket, key, version_id)
                        .await?;
                }
            }
            Err(StorageError::NotFound(_)) => {
                self.blobs.unlink_object(bucket, key).await?;
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }
}

// Private async helpers extracted from trait impl methods so timing wrappers
// in the trait impl can delegate to them from an inherent impl block.
impl ObjectStorage {
    async fn put_object_inner(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;

        if key.ends_with('/') {
            let prep = self.meta.fetch_put_bucket_context(bucket).await?;
            self.blobs.write_folder_marker(bucket, key).await?;
            let etag = "\"d41d8cd98f00b204e9800998ecf8427e\"".to_string();
            let mut meta = ObjectMeta {
                key: key.to_string(),
                size: 0,
                etag: etag.clone(),
                content_type: "application/x-directory".to_string(),
                last_modified: Self::now_ts(),
                owner_id: prep.owner_id.clone(),
                owner_display_name: prep.owner_display_name.clone(),
                acl: None,
                version_id: None,
                is_delete_marker: false,
                checksum_algorithm: None,
                checksum_value: None,
                tags: None,
                part_sizes: None,
            };
            normalize_object_meta(&mut meta, &prep.owner_id, &prep.owner_display_name);
            self.meta.upsert_object(bucket, &meta, Some(&prep)).await?;
            return Ok(PutResult {
                size: 0,
                etag,
                last_modified: Self::now_ts(),
                version_id: None,
                checksum_algorithm: None,
                checksum_value: None,
            });
        }

        let prep = self.meta.fetch_put_bucket_context(bucket).await?;
        let version_id = if prep.versioning {
            Some(BlobStorage::generate_version_id())
        } else {
            None
        };

        let written = self
            .blobs
            .write_flat_object_temp(bucket, key, body, checksum)
            .await?;

        let mut object_meta = ObjectMeta {
            key: key.to_string(),
            size: written.size,
            etag: written.etag.clone(),
            content_type: content_type.to_string(),
            last_modified: Self::now_ts(),
            owner_id: prep.owner_id.clone(),
            owner_display_name: prep.owner_display_name.clone(),
            acl: None,
            version_id: version_id.clone(),
            is_delete_marker: false,
            checksum_algorithm: written.checksum_algorithm,
            checksum_value: written.checksum_value.clone(),
            tags: None,
            part_sizes: None,
        };
        let owner_id = object_meta.owner_id.clone();
        let owner_name = object_meta.owner_display_name.clone();
        normalize_object_meta(&mut object_meta, &owner_id, &owner_name);

        self.finalize_written_object(
            bucket,
            key,
            object_meta,
            written,
            prep.versioning,
            Some(&prep),
        )
        .await
    }

    async fn delete_object_inner(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<DeleteResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;

        let versioned = self.meta.is_versioned(bucket).await.unwrap_or(false);
        if versioned {
            let version_id = BlobStorage::generate_version_id();
            let (owner_id, owner_name) = self.bucket_owner(bucket).await?;
            let marker = ObjectMeta {
                key: key.to_string(),
                size: 0,
                etag: String::new(),
                content_type: String::new(),
                last_modified: Self::now_ts(),
                owner_id,
                owner_display_name: owner_name,
                acl: None,
                version_id: Some(version_id.clone()),
                is_delete_marker: true,
                checksum_algorithm: None,
                checksum_value: None,
                tags: None,
                part_sizes: None,
            };
            self.meta.insert_version(bucket, &marker).await?;
            self.meta.delete_object_meta(bucket, key).await?;
            self.blobs.unlink_object(bucket, key).await?;
            return Ok(DeleteResult {
                version_id: Some(version_id),
                is_delete_marker: true,
            });
        }

        let (meta_result, blob_result) = tokio::join!(
            self.meta.delete_object_meta(bucket, key),
            self.blobs.unlink_object(bucket, key),
        );
        meta_result?;
        blob_result?;
        Ok(DeleteResult {
            version_id: None,
            is_delete_marker: false,
        })
    }

    async fn delete_objects_batch_inner(
        &self,
        bucket: &str,
        objects: &[BatchDeleteObject],
    ) -> Result<Vec<(BatchDeleteObject, Result<DeleteResult, StorageError>)>, StorageError> {
        if objects.is_empty() {
            return Ok(Vec::new());
        }
        validate_bucket_name(bucket)?;
        for obj in objects {
            validate_key(&obj.key)?;
        }

        let prep = self.meta.fetch_put_bucket_context(bucket).await?;
        let mut results: HashMap<(String, Option<String>), Result<DeleteResult, StorageError>> =
            HashMap::new();

        if prep.versioning {
            let mut version_pairs = Vec::new();
            let mut marker_keys = Vec::new();

            for obj in objects {
                match obj.version_id.as_deref() {
                    Some(vid) if vid != "null" => {
                        version_pairs.push((obj.key.clone(), vid.to_string()));
                    }
                    _ => marker_keys.push(obj.key.clone()),
                }
            }
            marker_keys.sort();
            marker_keys.dedup();

            if !version_pairs.is_empty() {
                let deleted = self
                    .meta
                    .delete_object_versions_batch(bucket, &version_pairs)
                    .await?;
                let deleted_set: HashSet<(String, String)> = deleted
                    .iter()
                    .map(|(k, v, _)| (k.clone(), v.clone()))
                    .collect();

                let mut current_keys = HashSet::new();
                let unlink_pairs: Vec<(String, String)> = deleted
                    .iter()
                    .map(|(k, v, _)| (k.clone(), v.clone()))
                    .collect();
                self.blobs
                    .unlink_version_blobs_batch(bucket, &unlink_pairs, DELETE_BLOB_CONCURRENCY)
                    .await?;
                for (key, vid, was_current) in &deleted {
                    if *was_current {
                        current_keys.insert(key.clone());
                    }
                    results.insert(
                        (key.clone(), Some(vid.clone())),
                        Ok(DeleteResult {
                            version_id: Some(vid.clone()),
                            is_delete_marker: false,
                        }),
                    );
                }
                for key in current_keys {
                    self.meta.update_current_after_delete(bucket, &key).await?;
                    self.sync_current_blobs_after_version_change(bucket, &key)
                        .await?;
                }

                for (key, vid) in &version_pairs {
                    if !deleted_set.contains(&(key.clone(), vid.clone())) {
                        results.insert(
                            (key.clone(), Some(vid.clone())),
                            Err(StorageError::VersionNotFound(vid.clone())),
                        );
                    }
                }
            }

            for key in marker_keys {
                let outcome = self.delete_object_inner(bucket, &key).await;
                results.insert((key.clone(), None), outcome);
            }
        } else {
            let keys: Vec<String> = objects.iter().map(|o| o.key.clone()).collect();
            let deleted = self.meta.delete_objects_by_keys(bucket, &keys).await?;
            let deleted_set: HashSet<String> = deleted.into_iter().collect();
            let to_unlink: Vec<String> = deleted_set.iter().cloned().collect();
            self.blobs
                .unlink_objects_batch(bucket, &to_unlink, DELETE_BLOB_CONCURRENCY)
                .await?;

            for obj in objects {
                let existed = deleted_set.contains(&obj.key);
                let outcome = if existed {
                    Ok(DeleteResult {
                        version_id: None,
                        is_delete_marker: false,
                    })
                } else {
                    Ok(DeleteResult {
                        version_id: None,
                        is_delete_marker: false,
                    })
                };
                results.insert((obj.key.clone(), obj.version_id.clone()), outcome);
            }
        }

        Ok(objects
            .iter()
            .map(|obj| {
                let outcome = results
                    .remove(&(obj.key.clone(), obj.version_id.clone()))
                    .unwrap_or_else(|| {
                        Ok(DeleteResult {
                            version_id: obj.version_id.clone(),
                            is_delete_marker: false,
                        })
                    });
                (obj.clone(), outcome)
            })
            .collect())
    }

    async fn upload_part_inner(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PartMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        if part_number == 0 || part_number > 10_000 {
            return Err(StorageError::InvalidKey(
                "part number must be 1..=10000".into(),
            ));
        }

        let upload = self.meta.get_multipart_upload(upload_id).await?;
        if upload.bucket != bucket {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }

        let (etag, size, checksum_algorithm, checksum_value) = match self
            .blobs
            .write_part(bucket, upload_id, part_number, body, checksum)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ = self
                    .blobs
                    .remove_part_file(bucket, upload_id, part_number)
                    .await;
                return Err(e);
            }
        };

        let part = PartMeta {
            part_number,
            etag,
            size,
            last_modified: Self::now_ts(),
            checksum_algorithm,
            checksum_value,
        };

        if let Err(e) = self.meta.upsert_part(upload_id, &part).await {
            let _ = self
                .blobs
                .remove_part_file(bucket, upload_id, part_number)
                .await;
            return Err(e);
        }

        Ok(part)
    }

    async fn complete_multipart_upload_inner(
        &self,
        bucket: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        if parts.is_empty() {
            return Err(StorageError::InvalidKey(
                "at least one part is required to complete upload".into(),
            ));
        }

        let upload_meta = self.meta.get_multipart_upload(upload_id).await?;
        if upload_meta.bucket != bucket {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }

        // list_parts returns rows already ordered by part_number; build a map for O(1) lookup.
        let all_parts: HashMap<u32, PartMeta> = self
            .meta
            .list_parts(upload_id)
            .await?
            .into_iter()
            .map(|p| (p.part_number, p))
            .collect();

        let mut selected = Vec::with_capacity(parts.len());
        for (idx, (part_number, requested_etag)) in parts.iter().enumerate() {
            let meta = all_parts
                .get(part_number)
                .cloned()
                .ok_or_else(|| StorageError::InvalidKey(format!("missing part {}", part_number)))?;
            if meta.etag != *requested_etag {
                return Err(StorageError::InvalidKey(format!(
                    "etag mismatch for part {}",
                    part_number
                )));
            }
            if idx + 1 < parts.len() && meta.size < 5 * 1024 * 1024 {
                return Err(StorageError::InvalidKey("part too small".into()));
            }
            selected.push(meta);
        }

        let prep = self.meta.fetch_put_bucket_context(bucket).await?;
        let version_id = if prep.versioning {
            Some(BlobStorage::generate_version_id())
        } else {
            None
        };

        let mut written = self
            .blobs
            .assemble_multipart_temp(bucket, &upload_meta.key, upload_id, &selected)
            .await?;

        let (checksum_algorithm, checksum_value) =
            if let Some(algo) = upload_meta.checksum_algorithm {
                (
                    Some(algo),
                    BlobStorage::composite_multipart_checksum(algo, &selected),
                )
            } else {
                (None, None)
            };
        written.checksum_algorithm = checksum_algorithm;
        written.checksum_value = checksum_value.clone();

        let part_sizes: Vec<u64> = selected.iter().map(|p| p.size).collect();
        let object_meta = ObjectMeta {
            key: upload_meta.key.clone(),
            size: written.size,
            etag: written.etag.clone(),
            content_type: upload_meta.content_type,
            last_modified: Self::now_ts(),
            owner_id: prep.owner_id.clone(),
            owner_display_name: prep.owner_display_name.clone(),
            acl: None,
            version_id: version_id.clone(),
            is_delete_marker: false,
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: Some(part_sizes),
        };

        let result = self
            .finalize_written_object(
                bucket,
                &upload_meta.key,
                object_meta,
                written,
                prep.versioning,
                Some(&prep),
            )
            .await?;

        self.meta.abort_multipart_upload(upload_id).await?;
        self.blobs.remove_upload_dir(bucket, upload_id).await?;

        Ok(result)
    }
}

#[async_trait]
impl Storage for ObjectStorage {
    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError> {
        let t = std::time::Instant::now();
        let result = self.meta.create_bucket(meta).await;
        self.record("create_bucket", t.elapsed(), 0);
        result
    }

    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError> {
        self.meta.head_bucket(name).await
    }

    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError> {
        let t = std::time::Instant::now();
        let result = self.meta.delete_bucket(name).await;
        if matches!(&result, Ok(true)) {
            let _ = self.blobs.remove_bucket_tree(name).await;
        }
        self.record("delete_bucket", t.elapsed(), 0);
        result
    }

    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError> {
        let t = std::time::Instant::now();
        let result = self.meta.list_buckets().await;
        self.record("list_buckets", t.elapsed(), 0);
        result
    }

    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError> {
        self.meta.put_bucket_policy(bucket, policy).await
    }

    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError> {
        self.meta.get_bucket_policy(bucket).await
    }

    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError> {
        self.meta.delete_bucket_policy(bucket).await
    }

    async fn put_bucket_acl(&self, bucket: &str, acl: crate::iam::Acl) -> Result<(), StorageError> {
        self.meta.put_bucket_acl(bucket, acl).await
    }

    async fn get_bucket_acl(&self, bucket: &str) -> Result<crate::iam::Acl, StorageError> {
        self.meta.get_bucket_acl(bucket).await
    }

    async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<CorsRule>,
    ) -> Result<(), StorageError> {
        self.meta.put_bucket_cors(bucket, rules).await
    }

    async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>, StorageError> {
        self.meta.get_bucket_cors(bucket).await
    }

    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError> {
        self.meta.delete_bucket_cors(bucket).await
    }

    async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError> {
        self.meta.is_versioned(bucket).await
    }

    async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError> {
        self.meta.set_versioning(bucket, enabled).await
    }

    async fn get_bucket_auth_info(
        &self,
        bucket: &str,
    ) -> Result<(Option<String>, crate::iam::Acl), StorageError> {
        let snap = self.meta.fetch_bucket_auth_context(bucket).await?;
        let acl = snap
            .acl
            .unwrap_or_else(|| crate::iam::Acl::private(&snap.owner_id, &snap.owner_display_name));
        Ok((snap.policy, acl))
    }

    async fn fetch_bucket_auth_context(
        &self,
        bucket: &str,
    ) -> Result<crate::db::repos::BucketAuthSnapshot, StorageError> {
        self.meta.fetch_bucket_auth_context(bucket).await
    }

    async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PutResult, StorageError> {
        let t = std::time::Instant::now();
        let result = self
            .put_object_inner(bucket, key, content_type, body, checksum)
            .await;
        let bytes = result.as_ref().map(|r| r.size).unwrap_or(0);
        self.record("put_object", t.elapsed(), bytes);
        result
    }

    async fn get_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        let t = std::time::Instant::now();
        let result = async {
            validate_bucket_name(bucket)?;
            validate_key(key)?;
            let meta = self.meta.get_object_for_read(bucket, key).await?;
            if meta.is_delete_marker {
                return Err(StorageError::NotFound(key.to_string()));
            }
            let stream = self.blobs.open_object(bucket, key, &meta).await?;
            Ok((stream, meta))
        }
        .await;
        let bytes = result.as_ref().map(|(_, meta)| meta.size).unwrap_or(0);
        self.record("get_object", t.elapsed(), bytes);
        result
    }

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        length: u64,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        let t = std::time::Instant::now();
        let result = async {
            validate_bucket_name(bucket)?;
            validate_key(key)?;
            let meta = self.meta.get_object_for_read(bucket, key).await?;
            if meta.is_delete_marker {
                return Err(StorageError::NotFound(key.to_string()));
            }
            let stream = self
                .blobs
                .open_object_range(bucket, key, &meta, offset, length)
                .await?;
            Ok((stream, meta))
        }
        .await;
        let bytes = if result.is_ok() { length } else { 0 };
        self.record("get_object_range", t.elapsed(), bytes);
        result
    }

    async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        let t = std::time::Instant::now();
        let result = async {
            validate_bucket_name(bucket)?;
            validate_key(key)?;
            let meta = self.meta.get_object_for_read(bucket, key).await?;
            if meta.is_delete_marker {
                return Err(StorageError::NotFound(key.to_string()));
            }
            Ok(meta)
        }
        .await;
        self.record("head_object", t.elapsed(), 0);
        result
    }

    async fn open_range(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
        offset: u64,
        length: u64,
    ) -> Result<ByteStream, StorageError> {
        self.blobs
            .open_object_range(bucket, key, meta, offset, length)
            .await
    }

    async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<HashMap<String, String>, StorageError> {
        validate_key(key)?;
        self.meta.get_object_tags(bucket, key).await
    }

    async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: HashMap<String, String>,
    ) -> Result<(), StorageError> {
        validate_key(key)?;
        self.meta.put_object_tags(bucket, key, tags).await
    }

    async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        self.meta.delete_object_tags(bucket, key).await
    }

    async fn delete_object(&self, bucket: &str, key: &str) -> Result<DeleteResult, StorageError> {
        let t = std::time::Instant::now();
        let result = self.delete_object_inner(bucket, key).await;
        self.record("delete_object", t.elapsed(), 0);
        result
    }

    async fn delete_objects_batch(
        &self,
        bucket: &str,
        objects: &[BatchDeleteObject],
    ) -> Result<Vec<(BatchDeleteObject, Result<DeleteResult, StorageError>)>, StorageError> {
        let t = std::time::Instant::now();
        let result = self.delete_objects_batch_inner(bucket, objects).await;
        self.record("delete_objects_batch", t.elapsed(), 0);
        result
    }

    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: Option<&str>,
        max_keys: usize,
        search: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        let t = std::time::Instant::now();
        let result = self
            .meta
            .list_objects_page(bucket, prefix, start_after, max_keys, search)
            .await;
        self.record("list_objects", t.elapsed(), 0);
        result
    }

    async fn put_object_acl(
        &self,
        bucket: &str,
        key: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError> {
        self.meta.put_object_acl(bucket, key, acl).await
    }

    async fn get_object_acl(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<crate::iam::Acl, StorageError> {
        self.meta.get_object_acl(bucket, key).await
    }

    async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        checksum_algorithm: Option<ChecksumAlgorithm>,
    ) -> Result<MultipartUploadMeta, StorageError> {
        let t = std::time::Instant::now();
        let result = async {
            validate_bucket_name(bucket)?;
            validate_key(key)?;
            let upload_id = uuid::Uuid::new_v4().to_string();
            self.blobs.ensure_upload_dir(bucket, &upload_id).await?;

            let meta = MultipartUploadMeta {
                upload_id: upload_id.clone(),
                bucket: bucket.to_string(),
                key: key.to_string(),
                content_type: content_type.to_string(),
                initiated: Self::now_ts(),
                checksum_algorithm,
            };
            self.meta.create_multipart_upload(&meta).await?;
            Ok(meta)
        }
        .await;
        self.record("create_multipart_upload", t.elapsed(), 0);
        result
    }

    async fn upload_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PartMeta, StorageError> {
        let t = std::time::Instant::now();
        let result = self
            .upload_part_inner(bucket, upload_id, part_number, body, checksum)
            .await;
        let bytes = result.as_ref().map(|part| part.size).unwrap_or(0);
        self.record("upload_part", t.elapsed(), bytes);
        result
    }

    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<PutResult, StorageError> {
        let t = std::time::Instant::now();
        let result = self
            .complete_multipart_upload_inner(bucket, upload_id, parts)
            .await;
        let bytes = result.as_ref().map(|r| r.size).unwrap_or(0);
        self.record("complete_multipart_upload", t.elapsed(), bytes);
        result
    }

    async fn get_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<MultipartUploadMeta, StorageError> {
        validate_upload_id(upload_id)?;
        self.meta.get_multipart_upload(upload_id).await
    }

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        let upload = self.meta.get_multipart_upload(upload_id).await?;
        if upload.bucket != bucket {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }
        self.meta.abort_multipart_upload(upload_id).await?;
        self.blobs.remove_upload_dir(bucket, upload_id).await
    }

    async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number_marker: Option<u32>,
        max_parts: usize,
    ) -> Result<(Vec<PartMeta>, bool), StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        let upload = self.meta.get_multipart_upload(upload_id).await?;
        if upload.bucket != bucket {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }

        let parts = self.meta.list_parts(upload_id).await?;
        let filtered: Vec<PartMeta> = parts
            .into_iter()
            .filter(|p| part_number_marker.is_none_or(|m| p.part_number > m))
            .collect();
        let is_truncated = filtered.len() > max_parts;
        let page = filtered.into_iter().take(max_parts).collect();
        Ok((page, is_truncated))
    }

    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        self.meta.list_multipart_uploads(bucket, prefix).await
    }

    async fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if version_id == "null" {
            return self.get_object(bucket, key).await;
        }
        let meta = self
            .meta
            .get_object_version_meta(bucket, key, version_id)
            .await?;
        if meta.is_delete_marker {
            return Err(StorageError::NotFound(key.to_string()));
        }
        let stream = self.blobs.open_version(bucket, key, version_id).await?;
        Ok((stream, meta))
    }

    async fn head_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if version_id == "null" {
            return self.head_object(bucket, key).await;
        }
        let meta = self
            .meta
            .get_object_version_meta(bucket, key, version_id)
            .await?;
        if meta.is_delete_marker {
            return Err(StorageError::NotFound(key.to_string()));
        }
        Ok(meta)
    }

    async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<DeleteResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;

        if version_id == "null" {
            let meta = match self.meta.get_object_meta(bucket, key).await {
                Ok(meta) => meta,
                Err(StorageError::NotFound(_)) => {
                    return Ok(DeleteResult {
                        version_id: None,
                        is_delete_marker: false,
                    });
                }
                Err(e) => return Err(e),
            };
            self.meta.delete_object_meta(bucket, key).await?;
            self.blobs.unlink_object(bucket, key).await?;
            self.meta.update_current_after_delete(bucket, key).await?;
            self.sync_current_blobs_after_version_change(bucket, key)
                .await?;
            return Ok(DeleteResult {
                version_id: meta.version_id,
                is_delete_marker: meta.is_delete_marker,
            });
        }

        let meta = self
            .meta
            .get_object_version_meta(bucket, key, version_id)
            .await?;
        self.meta
            .delete_object_version_meta(bucket, key, version_id)
            .await?;
        self.blobs
            .unlink_version_blobs(bucket, key, version_id)
            .await?;
        self.meta.update_current_after_delete(bucket, key).await?;
        self.sync_current_blobs_after_version_change(bucket, key)
            .await?;

        Ok(DeleteResult {
            version_id: Some(version_id.to_string()),
            is_delete_marker: meta.is_delete_marker,
        })
    }

    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        self.meta.list_object_versions(bucket, prefix).await
    }

    async fn list_object_versions_page(
        &self,
        bucket: &str,
        prefix: &str,
        key_marker: Option<&str>,
        version_id_marker: Option<&str>,
        max_keys: usize,
    ) -> Result<crate::db::repos::VersionsPage, StorageError> {
        validate_bucket_name(bucket)?;
        self.meta
            .list_object_versions_page(bucket, prefix, key_marker, version_id_marker, max_keys)
            .await
    }

    async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64) {
        let stale_before = chrono::Utc::now() - stale_after;
        let uploads_removed = self
            .meta
            .cleanup_stale_uploads(stale_before)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("housekeeping: stale upload cleanup failed: {}", e);
                0
            });
        let temp_removed = self.blobs.housekeeping_temp_sweep().await;
        (uploads_removed, temp_removed)
    }
}
