use super::chunk_reader::VerifiedChunkReader;
use super::{
    BucketMeta, ByteStream, ChecksumAlgorithm, ChunkInfo, ChunkKind, ChunkManifest, DeleteResult,
    MultipartUploadMeta, ObjectMeta, PartMeta, PutResult, StorageError, normalize_bucket_meta,
    normalize_object_meta, validate_bucket_name,
};
use base64::Engine;
use md5::{Digest, Md5};
use sha2::Sha256;
use rand::RngExt;
use std::path::{Component, Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter};

const IO_BUFFER_SIZE: usize = 256 * 1024;
const SMALL_OBJECT_THRESHOLD: u64 = 256 * 1024;

enum ChecksumHasher {
    Crc32(crc32fast::Hasher),
    Crc32c(u32),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
}

impl ChecksumHasher {
    fn new(algo: ChecksumAlgorithm) -> Self {
        match algo {
            ChecksumAlgorithm::CRC32 => Self::Crc32(crc32fast::Hasher::new()),
            ChecksumAlgorithm::CRC32C => Self::Crc32c(0),
            ChecksumAlgorithm::SHA1 => Self::Sha1(<sha1::Sha1 as Digest>::new()),
            ChecksumAlgorithm::SHA256 => Self::Sha256(<sha2::Sha256 as Digest>::new()),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Crc32(h) => h.update(data),
            Self::Crc32c(v) => *v = crc32c::crc32c_append(*v, data),
            Self::Sha1(h) => Digest::update(h, data),
            Self::Sha256(h) => Digest::update(h, data),
        }
    }

    fn finalize_base64(self) -> String {
        let b64 = base64::engine::general_purpose::STANDARD;
        match self {
            Self::Crc32(h) => b64.encode(h.finalize().to_be_bytes()),
            Self::Crc32c(v) => b64.encode(v.to_be_bytes()),
            Self::Sha1(h) => b64.encode(Digest::finalize(h)),
            Self::Sha256(h) => b64.encode(Digest::finalize(h)),
        }
    }
}

pub struct FilesystemStorage {
    buckets_dir: PathBuf,
    erasure_coding: bool,
    chunk_size: u64,
    parity_shards: u32,
}

/// Validate that an object key does not contain path traversal components.
fn validate_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty() {
        return Err(StorageError::InvalidKey("Key must not be empty".into()));
    }
    if key.len() > 1024 {
        return Err(StorageError::InvalidKey(
            "Key must not exceed 1024 bytes".into(),
        ));
    }
    let path = Path::new(key);
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(StorageError::InvalidKey(
                    "Key must not contain '..' path components".into(),
                ));
            }
            Component::RootDir => {
                return Err(StorageError::InvalidKey(
                    "Key must not be an absolute path".into(),
                ));
            }
            Component::Normal(seg) => {
                let name = seg.to_string_lossy();
                if is_reserved_segment(&name) {
                    return Err(StorageError::InvalidKey(format!(
                        "Key segment '{}' collides with an internal storage name",
                        name
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Reject key path segments that collide with internal on-disk names
/// (sidecars, bucket metadata, internal dirs, EC dirs, temp files).
fn is_reserved_segment(name: &str) -> bool {
    name.ends_with(".meta.json")
        || name.ends_with(".ec")
        || name == ".bucket.json"
        || name == ".uploads"
        || name == ".versions"
        || name == ".folder"
        || name.starts_with(".maxio-tmp-")
}

fn validate_upload_id(upload_id: &str) -> Result<(), StorageError> {
    if upload_id.is_empty() {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }
    if upload_id.contains('/') || upload_id.contains('\\') || upload_id.contains("..") {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }
    Ok(())
}


impl FilesystemStorage {
    pub async fn new(
        data_dir: &str,
        erasure_coding: bool,
        chunk_size: u64,
        parity_shards: u32,
    ) -> Result<Self, anyhow::Error> {
        let buckets_dir = Path::new(data_dir).join("buckets");
        fs::create_dir_all(&buckets_dir).await?;
        Ok(Self {
            buckets_dir,
            erasure_coding,
            chunk_size,
            parity_shards,
        })
    }

    // --- Bucket operations ---

    pub async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError> {
        validate_bucket_name(&meta.name)?;
        let bucket_dir = self.buckets_dir.join(&meta.name);
        match fs::create_dir(&bucket_dir).await {
            Ok(()) => {
                let meta_path = bucket_dir.join(".bucket.json");
                let json = serde_json::to_string_pretty(meta)?;
                if let Err(e) = fs::write(&meta_path, json).await {
                    // Clean up the empty directory to avoid a half-created bucket
                    let _ = fs::remove_dir(&bucket_dir).await;
                    return Err(e.into());
                }
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn head_bucket(&self, name: &str) -> Result<bool, StorageError> {
        validate_bucket_name(name)?;
        Ok(fs::try_exists(self.buckets_dir.join(name).join(".bucket.json")).await?)
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError> {
        validate_bucket_name(name)?;
        let bucket_dir = self.buckets_dir.join(name);
        if !fs::try_exists(&bucket_dir).await? {
            return Ok(false);
        }
        // Pass 1: read-only walk. Any real object (data file, `.folder`
        // marker, `.ec/` chunk dir) at any depth → BucketNotEmpty.
        if self.has_real_objects(&bucket_dir).await? {
            return Err(StorageError::BucketNotEmpty);
        }
        // Pass 2: purge sidecars (`*.meta.json`), internal dirs
        // (`.uploads`, `.versions`), and empty subdirs at any depth.
        self.purge_empty_bucket(&bucket_dir).await?;
        match fs::remove_dir(&bucket_dir).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                // Concurrent writer slipped a file in between passes.
                Err(StorageError::BucketNotEmpty)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Read-only check: does this bucket contain any real object data?
    fn has_real_objects<'a>(
        &'a self,
        dir: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = fs::read_dir(dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == ".bucket.json"
                    || name == ".uploads"
                    || name == ".versions"
                    || name.ends_with(".meta.json")
                {
                    continue;
                }
                let ft = entry.file_type().await?;
                if ft.is_dir() && name.ends_with(".ec") {
                    return Ok(true);
                }
                if ft.is_dir() {
                    if self.has_real_objects(&entry.path()).await? {
                        return Ok(true);
                    }
                } else {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }

    /// Post-order purge: remove sidecars / internal dirs at any depth and
    /// empty out every subdirectory. Must only run after
    /// `has_real_objects` returned `false`.
    fn purge_empty_bucket<'a>(
        &'a self,
        dir: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = fs::read_dir(dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();
                let ft = entry.file_type().await?;

                if name == ".bucket.json"
                    || name == ".uploads"
                    || name == ".versions"
                    || name.ends_with(".meta.json")
                {
                    if ft.is_dir() {
                        fs::remove_dir_all(&path).await?;
                    } else {
                        fs::remove_file(&path).await?;
                    }
                    continue;
                }

                if ft.is_dir() {
                    self.purge_empty_bucket(&path).await?;
                    fs::remove_dir(&path).await?;
                }
                // Non-sidecar regular files shouldn't exist here
                // (has_real_objects would have rejected) — skip defensively.
            }
            Ok(())
        })
    }

    pub async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError> {
        let mut buckets = Vec::new();
        let mut entries = fs::read_dir(&self.buckets_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(meta) = self.load_bucket_meta(&name).await {
                    buckets.push(meta);
                }
            }
        }
        buckets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(buckets)
    }

    async fn bucket_meta_path(&self, bucket: &str) -> PathBuf {
        self.buckets_dir.join(bucket).join(".bucket.json")
    }

    pub async fn load_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.bucket_meta_path(bucket).await;
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        if normalize_bucket_meta(&mut meta) {
            fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        }
        Ok(meta)
    }

    async fn save_bucket_meta(&self, meta: &BucketMeta) -> Result<(), StorageError> {
        validate_bucket_name(&meta.name)?;
        let meta_path = self.bucket_meta_path(&meta.name).await;
        fs::write(&meta_path, serde_json::to_string_pretty(meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError> {
        self.load_bucket_meta(bucket).await
    }

    pub async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError> {
        let mut meta = self.load_bucket_meta(bucket).await?;
        meta.policy = Some(policy.to_string());
        self.save_bucket_meta(&meta).await
    }

    pub async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError> {
        let meta = self.load_bucket_meta(bucket).await?;
        Ok(meta.policy)
    }

    pub async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError> {
        let mut meta = self.load_bucket_meta(bucket).await?;
        meta.policy = None;
        self.save_bucket_meta(&meta).await
    }

    pub async fn put_bucket_acl(
        &self,
        bucket: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError> {
        let mut meta = self.load_bucket_meta(bucket).await?;
        meta.acl = Some(acl);
        self.save_bucket_meta(&meta).await
    }

    pub async fn get_bucket_acl(&self, bucket: &str) -> Result<crate::iam::Acl, StorageError> {
        let meta = self.load_bucket_meta(bucket).await?;
        Ok(meta.acl.unwrap_or_else(|| {
            crate::iam::Acl::private(&meta.owner_id, &meta.owner_display_name)
        }))
    }

    pub async fn put_object_acl(
        &self,
        bucket: &str,
        key: &str,
        acl: crate::iam::Acl,
    ) -> Result<(), StorageError> {
        let meta_path = self.meta_path(bucket, key);
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: ObjectMeta = serde_json::from_str(&data)?;
        meta.acl = Some(acl);
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn get_object_acl(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<crate::iam::Acl, StorageError> {
        let meta_path = self.meta_path(bucket, key);
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: ObjectMeta = serde_json::from_str(&data)?;
        if let Some(acl) = meta.acl {
            return Ok(acl);
        }
        Ok(crate::iam::Acl::private(
            &meta.owner_id,
            &meta.owner_display_name,
        ))
    }

    // --- Object operations ---

    fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir.join(bucket).join(dir).join(".folder")
        } else {
            self.buckets_dir.join(bucket).join(key)
        }
    }

    fn meta_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir
                .join(bucket)
                .join(dir)
                .join(".folder.meta.json")
        } else {
            self.buckets_dir
                .join(bucket)
                .join(format!("{}.meta.json", key))
        }
    }

    fn ec_dir(&self, bucket: &str, key: &str) -> PathBuf {
        self.buckets_dir.join(bucket).join(format!("{}.ec", key))
    }

    fn manifest_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.ec_dir(bucket, key).join("manifest.json")
    }

    async fn is_chunked_path(ec_dir: &Path) -> bool {
        matches!(fs::metadata(ec_dir).await, Ok(m) if m.is_dir())
    }

    async fn read_manifest(&self, bucket: &str, key: &str) -> Result<ChunkManifest, StorageError> {
        let path = self.manifest_path(bucket, key);
        let data = fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    fn uploads_dir(&self, bucket: &str) -> PathBuf {
        self.buckets_dir.join(bucket).join(".uploads")
    }

    fn upload_dir(&self, bucket: &str, upload_id: &str) -> PathBuf {
        self.uploads_dir(bucket).join(upload_id)
    }

    fn upload_meta_path(&self, bucket: &str, upload_id: &str) -> PathBuf {
        self.upload_dir(bucket, upload_id).join(".meta.json")
    }

    fn part_path(&self, bucket: &str, upload_id: &str, part_number: u32) -> PathBuf {
        self.upload_dir(bucket, upload_id)
            .join(part_number.to_string())
    }

    fn part_meta_path(&self, bucket: &str, upload_id: &str, part_number: u32) -> PathBuf {
        self.upload_dir(bucket, upload_id)
            .join(format!("{}.meta.json", part_number))
    }

    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        mut body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;

        if key.ends_with('/') {
            return self.put_folder_marker(bucket, key).await;
        }

        if self.erasure_coding {
            return self
                .put_object_chunked(
                    bucket,
                    key,
                    content_type,
                    body,
                    checksum.as_ref().map(|(a, _)| *a),
                )
                .await;
        }

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let obj_path = self.object_path(bucket, key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let tmp_obj_path = temp_sibling_path(&obj_path);
        let mut tmp_obj_guard = TempPathGuard::file(tmp_obj_path.clone());
        let file = fs::File::create(&tmp_obj_path).await?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, file);
        let mut hasher = Md5::new();
        let mut checksum_hasher = checksum
            .as_ref()
            .map(|(algo, _)| ChecksumHasher::new(*algo));
        let mut size: u64 = 0;
        let mut buf = vec![0u8; IO_BUFFER_SIZE];

        loop {
            let n = body.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            if let Some(ref mut ch) = checksum_hasher {
                ch.update(&buf[..n]);
            }
            size += n as u64;
            writer.write_all(&buf[..n]).await?;
        }
        writer.flush().await?;

        let etag = hex::encode(hasher.finalize());
        let etag_quoted = format!("\"{}\"", etag);

        let (checksum_algorithm, checksum_value) = if let Some((algo, expected)) = checksum {
            let computed = checksum_hasher.unwrap().finalize_base64();
            if let Some(expected_val) = expected {
                if computed != expected_val {
                    let _ = fs::remove_file(&tmp_obj_path).await;
                    return Err(StorageError::ChecksumMismatch(format!(
                        "expected {}, got {}",
                        expected_val, computed
                    )));
                }
            }
            (Some(algo), Some(computed))
        } else {
            (None, None)
        };

        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let mut meta = ObjectMeta {
            key: key.to_string(),
            size,
            etag: etag_quoted.clone(),
            content_type: content_type.to_string(),
            last_modified: now,
            owner_id: String::new(),
            owner_display_name: String::new(),
            acl: None,
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: None,
        };

        let bucket_meta = self.load_bucket_meta(bucket).await?;
        normalize_object_meta(&mut meta, &bucket_meta.owner_id, &bucket_meta.owner_display_name);

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(&meta)?;
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, json).await?;
        publish_temp_payload_and_meta(&tmp_obj_path, &obj_path, false, &tmp_meta_path, &meta_path)
            .await?;
        tmp_obj_guard.disarm();
        tmp_meta_guard.disarm();

        if versioned {
            self.write_version(bucket, key, &meta, &obj_path).await?;
        }

        Ok(PutResult {
            size,
            etag: etag_quoted,
            version_id,
            checksum_algorithm,
            checksum_value,
        })
    }

    async fn put_object_chunked(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        mut body: ByteStream,
        checksum_algo: Option<ChecksumAlgorithm>,
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        let ec_dir = self.ec_dir(bucket, key);
        let tmp_ec_dir = temp_sibling_path(&ec_dir);
        let mut tmp_ec_guard = TempPathGuard::dir(tmp_ec_dir.clone());
        if let Some(parent) = ec_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::create_dir_all(&tmp_ec_dir).await?;

        let mut md5_hasher = Md5::new();
        let mut checksum_hasher = checksum_algo.map(ChecksumHasher::new);
        let mut total_size: u64 = 0;
        let mut chunks: Vec<ChunkInfo> = Vec::new();
        let mut chunk_index: u32 = 0;

        let mut read_buf = vec![0u8; IO_BUFFER_SIZE];
        let mut chunk_buf = Vec::with_capacity(self.chunk_size as usize);

        loop {
            let n = body.read(&mut read_buf).await?;
            if n == 0 {
                // Flush remaining chunk_buf
                if !chunk_buf.is_empty() {
                    let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_buf).await?;
                    chunks.push(ci);
                }
                break;
            }

            md5_hasher.update(&read_buf[..n]);
            if let Some(ref mut ch) = checksum_hasher {
                ch.update(&read_buf[..n]);
            }
            total_size += n as u64;
            chunk_buf.extend_from_slice(&read_buf[..n]);

            while chunk_buf.len() >= self.chunk_size as usize {
                let chunk_data: Vec<u8> = chunk_buf.drain(..self.chunk_size as usize).collect();
                let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_data).await?;
                chunks.push(ci);
                chunk_index += 1;
            }
        }

        // Handle empty object (zero chunks)
        if chunks.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, 0, &[]).await?;
            chunks.push(ci);
        }

        let data_chunk_count = chunks.len() as u32;

        // Compute and write parity shards if configured (skip for empty objects)
        let has_parity = self.parity_shards > 0 && total_size > 0;
        if has_parity {
            let parity_infos = self
                .compute_and_write_parity_in_dir(&tmp_ec_dir, &chunks)
                .await?;
            chunks.extend(parity_infos);
        }

        let manifest = ChunkManifest {
            version: if has_parity { 2 } else { 1 },
            total_size,
            chunk_size: self.chunk_size,
            chunk_count: data_chunk_count,
            chunks,
            parity_shards: if has_parity {
                Some(self.parity_shards)
            } else {
                None
            },
            shard_size: if has_parity {
                Some(self.chunk_size)
            } else {
                None
            },
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        fs::write(tmp_ec_dir.join("manifest.json"), manifest_json).await?;

        let etag = hex::encode(md5_hasher.finalize());
        let etag_quoted = format!("\"{}\"", etag);
        let checksum_value = checksum_hasher.map(|h| h.finalize_base64());

        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let storage_format = if has_parity {
            "chunked-v2"
        } else {
            "chunked-v1"
        };
        let meta = ObjectMeta {
            key: key.to_string(),
            size: total_size,
            etag: etag_quoted.clone(),
            content_type: content_type.to_string(),
            last_modified: now,
            owner_id: String::new(),
            owner_display_name: String::new(),
            acl: None,
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: Some(storage_format.to_string()),
            checksum_algorithm: checksum_algo,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: None,
        };

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&meta)?).await?;
        publish_temp_payload_and_meta(&tmp_ec_dir, &ec_dir, true, &tmp_meta_path, &meta_path)
            .await?;
        tmp_ec_guard.disarm();
        tmp_meta_guard.disarm();

        if versioned {
            self.write_version_chunked(bucket, key, &meta).await?;
        }

        Ok(PutResult {
            size: total_size,
            etag: etag_quoted,
            version_id,
            checksum_algorithm: checksum_algo,
            checksum_value,
        })
    }


    async fn compute_and_write_parity_in_dir(
        &self,
        dir: &Path,
        data_chunks: &[ChunkInfo],
    ) -> Result<Vec<ChunkInfo>, StorageError> {
        self.compute_and_write_parity_from(dir, data_chunks).await
    }

    async fn compute_and_write_parity_from(
        &self,
        dir: &Path,
        data_chunks: &[ChunkInfo],
    ) -> Result<Vec<ChunkInfo>, StorageError> {
        use reed_solomon_erasure::galois_8::ReedSolomon;

        let k = data_chunks.len();
        let m = self.parity_shards as usize;

        if k + m > 255 {
            return Err(StorageError::InvalidKey(format!(
                "too many shards: {} data + {} parity = {} > 255 (GF(2^8) limit). Increase --chunk-size",
                k,
                m,
                k + m
            )));
        }

        let shard_size = self.chunk_size as usize;
        let mut all_shards: Vec<Vec<u8>> = Vec::with_capacity(k + m);
        for ci in data_chunks {
            let path = dir.join(format!("{:06}", ci.index));
            let mut data = std::fs::read(&path).map_err(StorageError::Io)?;
            data.resize(shard_size, 0u8);
            all_shards.push(data);
        }
        for _ in 0..m {
            all_shards.push(vec![0u8; shard_size]);
        }
        let rs = ReedSolomon::new(k, m)
            .map_err(|e| StorageError::InvalidKey(format!("Reed-Solomon init error: {e}")))?;
        rs.encode(&mut all_shards)
            .map_err(|e| StorageError::InvalidKey(format!("Reed-Solomon encode error: {e}")))?;

        let mut parity_infos = Vec::with_capacity(m);
        for i in 0..m {
            let parity_index = k as u32 + i as u32;
            let shard = &all_shards[k + i];
            let path = dir.join(format!("{:06}", parity_index));
            parity_infos.push(
                write_chunk_file(&path, parity_index, shard)
                    .await?
                    .into_parity(),
            );
        }
        Ok(parity_infos)
    }
}

trait ChunkInfoExt {
    fn into_parity(self) -> ChunkInfo;
}

impl ChunkInfoExt for ChunkInfo {
    fn into_parity(mut self) -> ChunkInfo {
        self.kind = ChunkKind::Parity;
        self
    }
}

async fn write_chunk_to_dir(
    dir: &Path,
    index: u32,
    data: &[u8],
) -> Result<ChunkInfo, StorageError> {
    write_chunk_file(&dir.join(format!("{:06}", index)), index, data).await
}

async fn write_chunk_file(path: &Path, index: u32, data: &[u8]) -> Result<ChunkInfo, StorageError> {
    let sha256 = hex::encode(Sha256::digest(data));
    let mut file = fs::File::create(&path).await?;
    file.write_all(data).await?;
    file.flush().await?;
    Ok(ChunkInfo {
        index,
        size: data.len() as u64,
        sha256,
        kind: ChunkKind::Data,
    })
}

fn temp_sibling_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".maxio-tmp-{}", uuid::Uuid::new_v4()))
}

struct TempPathGuard {
    path: PathBuf,
    is_dir: bool,
    armed: bool,
}

impl TempPathGuard {
    fn file(path: PathBuf) -> Self {
        Self {
            path,
            is_dir: false,
            armed: true,
        }
    }

    fn dir(path: PathBuf) -> Self {
        Self {
            path,
            is_dir: true,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        if self.armed {
            if self.is_dir {
                let _ = std::fs::remove_dir_all(&self.path);
            } else {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

async fn publish_temp_payload_and_meta(
    tmp_payload: &Path,
    final_payload: &Path,
    payload_is_dir: bool,
    tmp_meta: &Path,
    final_meta: &Path,
) -> Result<(), StorageError> {
    if let Some(parent) = final_payload.parent() {
        fs::create_dir_all(parent).await?;
    }
    if let Some(parent) = final_meta.parent() {
        fs::create_dir_all(parent).await?;
    }

    let payload_backup = backup_existing(final_payload).await?;
    let meta_backup = backup_existing(final_meta).await?;

    if let Err(e) = fs::rename(tmp_payload, final_payload).await {
        restore_backup(final_meta, &meta_backup, false).await;
        restore_backup(final_payload, &payload_backup, payload_is_dir).await;
        return Err(StorageError::Io(e));
    }

    if let Err(e) = fs::rename(tmp_meta, final_meta).await {
        remove_path_if_exists(final_payload, payload_is_dir).await;
        restore_backup(final_meta, &meta_backup, false).await;
        restore_backup(final_payload, &payload_backup, payload_is_dir).await;
        return Err(StorageError::Io(e));
    }

    cleanup_backup(&payload_backup, payload_is_dir).await;
    cleanup_backup(&meta_backup, false).await;
    Ok(())
}

async fn backup_existing(path: &Path) -> Result<Option<PathBuf>, StorageError> {
    if !fs::try_exists(path).await? {
        return Ok(None);
    }
    let backup = temp_sibling_path(path);
    fs::rename(path, &backup).await?;
    Ok(Some(backup))
}

async fn restore_backup(final_path: &Path, backup: &Option<PathBuf>, is_dir: bool) {
    if let Some(backup) = backup {
        remove_path_if_exists(final_path, is_dir).await;
        let _ = fs::rename(backup, final_path).await;
    }
}

async fn cleanup_backup(backup: &Option<PathBuf>, is_dir: bool) {
    if let Some(backup) = backup {
        remove_path_if_exists(backup, is_dir).await;
    }
}

async fn remove_path_if_exists(path: &Path, is_dir: bool) {
    if is_dir {
        let _ = fs::remove_dir_all(path).await;
    } else {
        let _ = fs::remove_file(path).await;
    }
}

impl FilesystemStorage {
    async fn complete_multipart_chunked(
        &self,
        bucket: &str,
        upload_id: &str,
        upload_meta: &MultipartUploadMeta,
        selected: &[PartMeta],
    ) -> Result<PutResult, StorageError> {
        let key = &upload_meta.key;
        let ec_dir = self.ec_dir(bucket, key);
        let tmp_ec_dir = temp_sibling_path(&ec_dir);
        let mut tmp_ec_guard = TempPathGuard::dir(tmp_ec_dir.clone());
        if let Some(parent) = ec_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::create_dir_all(&tmp_ec_dir).await?;
        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let mut total_size = 0u64;
        let mut etag_hasher = Md5::new();
        let mut chunks: Vec<ChunkInfo> = Vec::new();
        let mut chunk_index: u32 = 0;
        let mut chunk_buf = Vec::with_capacity(self.chunk_size as usize);

        let mut buf = vec![0u8; IO_BUFFER_SIZE];
        for part in selected {
            let mut part_file =
                fs::File::open(self.part_path(bucket, upload_id, part.part_number)).await?;
            loop {
                let n = part_file.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                total_size += n as u64;
                chunk_buf.extend_from_slice(&buf[..n]);

                while chunk_buf.len() >= self.chunk_size as usize {
                    let chunk_data: Vec<u8> = chunk_buf.drain(..self.chunk_size as usize).collect();
                    let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_data).await?;
                    chunks.push(ci);
                    chunk_index += 1;
                }
            }

            let raw_md5 = hex::decode(part.etag.trim_matches('"'))
                .map_err(|_| StorageError::InvalidKey("invalid part etag".into()))?;
            etag_hasher.update(raw_md5);
        }

        // Flush remaining
        if !chunk_buf.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_buf).await?;
            chunks.push(ci);
        }

        if chunks.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, 0, &[]).await?;
            chunks.push(ci);
        }

        let data_chunk_count = chunks.len() as u32;

        // Compute and write parity shards if configured (skip for empty objects)
        let has_parity = self.parity_shards > 0 && total_size > 0;
        if has_parity {
            let parity_infos = self
                .compute_and_write_parity_in_dir(&tmp_ec_dir, &chunks)
                .await?;
            chunks.extend(parity_infos);
        }

        let manifest = ChunkManifest {
            version: if has_parity { 2 } else { 1 },
            total_size,
            chunk_size: self.chunk_size,
            chunk_count: data_chunk_count,
            chunks,
            parity_shards: if has_parity {
                Some(self.parity_shards)
            } else {
                None
            },
            shard_size: if has_parity {
                Some(self.chunk_size)
            } else {
                None
            },
        };
        fs::write(
            tmp_ec_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )
        .await?;

        let etag = format!(
            "\"{}-{}\"",
            hex::encode(etag_hasher.finalize()),
            selected.len()
        );

        // Compute composite checksum if algorithm was specified
        let (checksum_algorithm, checksum_value) =
            if let Some(algo) = upload_meta.checksum_algorithm {
                let b64 = base64::engine::general_purpose::STANDARD;
                let mut raw_checksums = Vec::new();
                for part in selected {
                    if let Some(ref val) = part.checksum_value {
                        if let Ok(raw) = b64.decode(val) {
                            raw_checksums.extend_from_slice(&raw);
                        }
                    }
                }
                if !raw_checksums.is_empty() {
                    let mut composite_hasher = ChecksumHasher::new(algo);
                    composite_hasher.update(&raw_checksums);
                    let composite =
                        format!("{}-{}", composite_hasher.finalize_base64(), selected.len());
                    (Some(algo), Some(composite))
                } else {
                    (Some(algo), None)
                }
            } else {
                (None, None)
            };

        let part_sizes: Vec<u64> = selected.iter().map(|p| p.size).collect();
        let storage_format = if has_parity {
            "chunked-v2"
        } else {
            "chunked-v1"
        };
        let object_meta = ObjectMeta {
            key: key.to_string(),
            size: total_size,
            etag: etag.clone(),
            content_type: upload_meta.content_type.clone(),
            last_modified: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            owner_id: String::new(),
            owner_display_name: String::new(),
            acl: None,
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: Some(storage_format.to_string()),
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: Some(part_sizes),
        };

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&object_meta)?).await?;
        publish_temp_payload_and_meta(&tmp_ec_dir, &ec_dir, true, &tmp_meta_path, &meta_path)
            .await?;
        tmp_ec_guard.disarm();
        tmp_meta_guard.disarm();
        if versioned {
            self.write_version_chunked(bucket, key, &object_meta)
                .await?;
        }
        fs::remove_dir_all(self.upload_dir(bucket, upload_id)).await?;

        Ok(PutResult {
            size: total_size,
            etag,
            version_id,
            checksum_algorithm,
            checksum_value,
        })
    }

    /// Encrypt-then-EC multipart completion. Reads each part with the

    async fn put_folder_marker(&self, bucket: &str, key: &str) -> Result<PutResult, StorageError> {
        let folder_dir = self
            .buckets_dir
            .join(bucket)
            .join(key.trim_end_matches('/'));
        fs::create_dir_all(&folder_dir).await?;

        let marker_path = folder_dir.join(".folder");
        fs::write(&marker_path, b"").await?;

        let etag = "\"d41d8cd98f00b204e9800998ecf8427e\"".to_string();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let meta = ObjectMeta {
            key: key.to_string(),
            size: 0,
            etag: etag.clone(),
            content_type: "application/x-directory".to_string(),
            last_modified: now,
            owner_id: String::new(),
            owner_display_name: String::new(),
            acl: None,
            version_id: None,
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
        };

        let meta_path = folder_dir.join(".folder.meta.json");
        let json = serde_json::to_string_pretty(&meta)?;
        fs::write(&meta_path, json).await?;

        Ok(PutResult {
            size: 0,
            etag,
            version_id: None,
            checksum_algorithm: None,
            checksum_value: None,
        })
    }

    pub async fn get_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        let ec_dir = self.ec_dir(bucket, key);
        if Self::is_chunked_path(&ec_dir).await {
            let manifest = self.read_manifest(bucket, key).await?;
            let reader = VerifiedChunkReader::new(ec_dir, manifest);
            return Ok((Box::pin(reader), meta));
        }
        let obj_path = self.object_path(bucket, key);
        if meta.size <= SMALL_OBJECT_THRESHOLD {
            let data = fs::read(&obj_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::NotFound(key.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            return Ok((Box::pin(std::io::Cursor::new(data)), meta));
        }
        let file = fs::File::open(&obj_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let reader = BufReader::with_capacity(IO_BUFFER_SIZE, file);
        Ok((Box::pin(reader), meta))
    }

    pub async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        length: u64,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        let ec_dir = self.ec_dir(bucket, key);
        if Self::is_chunked_path(&ec_dir).await {
            let manifest = self.read_manifest(bucket, key).await?;
            let reader = VerifiedChunkReader::with_range(ec_dir, manifest, offset, length);
            return Ok((Box::pin(reader), meta));
        }
        let obj_path = self.object_path(bucket, key);
        if length <= SMALL_OBJECT_THRESHOLD {
            let mut file = fs::File::open(&obj_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::NotFound(key.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(StorageError::Io)?;
            let mut data = vec![0u8; length as usize];
            file.read_exact(&mut data).await.map_err(StorageError::Io)?;
            return Ok((Box::pin(std::io::Cursor::new(data)), meta));
        }
        let mut file = fs::File::open(&obj_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(StorageError::Io)?;
        let limited = file.take(length);
        let reader = BufReader::with_capacity(IO_BUFFER_SIZE, limited);
        Ok((Box::pin(reader), meta))
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        Ok(meta)
    }

    pub async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<std::collections::HashMap<String, String>, StorageError> {
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        Ok(meta.tags.unwrap_or_default())
    }

    pub async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: std::collections::HashMap<String, String>,
    ) -> Result<(), StorageError> {
        validate_key(key)?;
        let mut meta = self.read_object_meta(bucket, key).await?;
        meta.tags = if tags.is_empty() { None } else { Some(tags) };
        let json = serde_json::to_string_pretty(&meta)?;
        fs::write(self.meta_path(bucket, key), json).await?;
        Ok(())
    }

    pub async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        let mut meta = self.read_object_meta(bucket, key).await?;
        meta.tags = None;
        let json = serde_json::to_string_pretty(&meta)?;
        fs::write(self.meta_path(bucket, key), json).await?;
        Ok(())
    }

    pub async fn delete_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<DeleteResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        if versioned {
            return self.write_delete_marker(bucket, key).await;
        }

        let obj_path = self.object_path(bucket, key);
        let meta_path = self.meta_path(bucket, key);
        let ec_dir = self.ec_dir(bucket, key);

        if !fs::try_exists(&meta_path).await?
            && !fs::try_exists(&obj_path).await?
            && !fs::try_exists(&ec_dir).await?
        {
            return Ok(DeleteResult {
                version_id: None,
                is_delete_marker: false,
            });
        }
        remove_file_if_exists(&obj_path).await?;
        remove_file_if_exists(&meta_path).await?;
        remove_dir_all_if_exists(&ec_dir).await?;

        // Clean up empty parent directories (but not the bucket dir itself)
        let bucket_dir = self.buckets_dir.join(bucket);
        let mut dir = obj_path.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            if d == bucket_dir {
                break;
            }
            match fs::remove_dir(&d).await {
                Ok(()) => {}
                Err(_) => break,
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }

        Ok(DeleteResult {
            version_id: None,
            is_delete_marker: false,
        })
    }

    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        let bucket_dir = self.buckets_dir.join(bucket);
        let mut results = Vec::new();
        self.walk_dir(&bucket_dir, &bucket_dir, prefix, &mut results)
            .await?;
        results.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(results)
    }

    pub async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        checksum_algorithm: Option<ChecksumAlgorithm>,
    ) -> Result<MultipartUploadMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let upload_id = uuid::Uuid::new_v4().to_string();
        let upload_dir = self.upload_dir(bucket, &upload_id);
        fs::create_dir_all(&upload_dir).await?;

        let meta = MultipartUploadMeta {
            upload_id: upload_id.clone(),
            bucket: bucket.to_string(),
            key: key.to_string(),
            content_type: content_type.to_string(),
            initiated: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            checksum_algorithm,
        };

        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(self.upload_meta_path(bucket, &upload_id), meta_json).await?;
        Ok(meta)
    }

    pub async fn upload_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        mut body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<PartMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        if part_number == 0 || part_number > 10_000 {
            return Err(StorageError::InvalidKey(
                "part number must be 1..=10000".into(),
            ));
        }
        let upload_dir = self.upload_dir(bucket, upload_id);
        if !fs::try_exists(&upload_dir).await? {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }

        let part_path = self.part_path(bucket, upload_id, part_number);
        let file = fs::File::create(&part_path).await?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, file);
        let mut hasher = Md5::new();
        let mut checksum_hasher = checksum
            .as_ref()
            .map(|(algo, _)| ChecksumHasher::new(*algo));
        let mut size: u64 = 0;
        let mut buf = vec![0u8; IO_BUFFER_SIZE];

        loop {
            let n = body.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            if let Some(ref mut ch) = checksum_hasher {
                ch.update(&buf[..n]);
            }
            size += n as u64;
            writer.write_all(&buf[..n]).await?;
        }
        writer.flush().await?;

        let (checksum_algorithm, checksum_value) = if let Some((algo, expected)) = checksum {
            let computed = checksum_hasher.unwrap().finalize_base64();
            if let Some(expected_val) = expected {
                if computed != expected_val {
                    let _ = fs::remove_file(&part_path).await;
                    return Err(StorageError::ChecksumMismatch(format!(
                        "expected {}, got {}",
                        expected_val, computed
                    )));
                }
            }
            (Some(algo), Some(computed))
        } else {
            (None, None)
        };

        let etag = format!("\"{}\"", hex::encode(hasher.finalize()));
        let meta = PartMeta {
            part_number,
            etag,
            size,
            last_modified: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            checksum_algorithm,
            checksum_value,
        };
        if let Err(e) = fs::write(
            self.part_meta_path(bucket, upload_id, part_number),
            serde_json::to_string_pretty(&meta)?,
        )
        .await
        {
            let _ = fs::remove_file(&part_path).await;
            return Err(e.into());
        }
        Ok(meta)
    }

    pub async fn complete_multipart_upload(
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

        let upload_meta = self.read_upload_meta(bucket, upload_id).await?;
        let mut selected = Vec::with_capacity(parts.len());
        for (idx, (part_number, requested_etag)) in parts.iter().enumerate() {
            let meta = self.read_part_meta(bucket, upload_id, *part_number).await?;
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

        if self.erasure_coding {
            return self
                .complete_multipart_chunked(bucket, upload_id, &upload_meta, &selected)
                .await;
        }

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let obj_path = self.object_path(bucket, &upload_meta.key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_obj_path = temp_sibling_path(&obj_path);
        let mut tmp_obj_guard = TempPathGuard::file(tmp_obj_path.clone());
        let out = fs::File::create(&tmp_obj_path).await?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, out);
        let mut total_size = 0u64;
        let mut etag_hasher = Md5::new();
        let mut buf = vec![0u8; IO_BUFFER_SIZE];

        for part in &selected {
            let part_path = self.part_path(bucket, upload_id, part.part_number);
            let mut part_stream: ByteStream = Box::pin(fs::File::open(&part_path).await?);
            loop {
                let n = part_stream.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                total_size += n as u64;
                writer.write_all(&buf[..n]).await?;
            }

            let raw_md5 = hex::decode(part.etag.trim_matches('"'))
                .map_err(|_| StorageError::InvalidKey("invalid part etag".into()))?;
            etag_hasher.update(raw_md5);
        }
        writer.flush().await?;

        let etag = format!(
            "\"{}-{}\"",
            hex::encode(etag_hasher.finalize()),
            selected.len()
        );

        let (checksum_algorithm, checksum_value) =
            if let Some(algo) = upload_meta.checksum_algorithm {
                let b64 = base64::engine::general_purpose::STANDARD;
                let mut raw_checksums = Vec::new();
                for part in &selected {
                    if let Some(ref val) = part.checksum_value {
                        if let Ok(raw) = b64.decode(val) {
                            raw_checksums.extend_from_slice(&raw);
                        }
                    }
                }
                if !raw_checksums.is_empty() {
                    let mut composite_hasher = ChecksumHasher::new(algo);
                    composite_hasher.update(&raw_checksums);
                    let composite =
                        format!("{}-{}", composite_hasher.finalize_base64(), selected.len());
                    (Some(algo), Some(composite))
                } else {
                    (Some(algo), None)
                }
            } else {
                (None, None)
            };

        let part_sizes: Vec<u64> = selected.iter().map(|p| p.size).collect();
        let object_meta = ObjectMeta {
            key: upload_meta.key.clone(),
            size: total_size,
            etag: etag.clone(),
            content_type: upload_meta.content_type,
            last_modified: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            owner_id: String::new(),
            owner_display_name: String::new(),
            acl: None,
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: Some(part_sizes),
        };
        let meta_path = self.meta_path(bucket, &upload_meta.key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&object_meta)?).await?;
        publish_temp_payload_and_meta(&tmp_obj_path, &obj_path, false, &tmp_meta_path, &meta_path)
            .await?;
        tmp_obj_guard.disarm();
        tmp_meta_guard.disarm();
        if versioned {
            self.write_version(bucket, &upload_meta.key, &object_meta, &obj_path)
                .await?;
        }
        fs::remove_dir_all(self.upload_dir(bucket, upload_id)).await?;

        Ok(PutResult {
            size: total_size,
            etag,
            version_id,
            checksum_algorithm,
            checksum_value,
        })
    }

    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        let upload_dir = self.upload_dir(bucket, upload_id);
        if !fs::try_exists(&upload_dir).await? {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }
        fs::remove_dir_all(upload_dir).await?;
        Ok(())
    }

    pub async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(MultipartUploadMeta, Vec<PartMeta>), StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        let meta = self.read_upload_meta(bucket, upload_id).await?;
        let upload_dir = self.upload_dir(bucket, upload_id);
        let mut entries = fs::read_dir(&upload_dir).await?;
        let mut parts = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".meta.json") || name == ".meta.json" {
                continue;
            }
            let data = fs::read_to_string(entry.path()).await?;
            if let Ok(pm) = serde_json::from_str::<PartMeta>(&data) {
                parts.push(pm);
            }
        }
        parts.sort_by_key(|p| p.part_number);
        Ok((meta, parts))
    }

    pub async fn list_multipart_uploads(
        &self,
        bucket: &str,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        let uploads_dir = self.uploads_dir(bucket);
        if !fs::try_exists(&uploads_dir).await? {
            return Ok(Vec::new());
        }
        let mut entries = fs::read_dir(&uploads_dir).await?;
        let mut uploads = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            let upload_id = entry.file_name().to_string_lossy().to_string();
            if let Ok(meta) = self.read_upload_meta(bucket, &upload_id).await {
                uploads.push(meta);
            }
        }
        uploads.sort_by(|a, b| a.initiated.cmp(&b.initiated));
        Ok(uploads)
    }

    /// Background housekeeping: abort multipart uploads whose `.meta.json`
    /// `initiated` timestamp is older than `stale_after`, and remove
    /// leftover `.maxio-tmp-*` files from crashed writes. Returns
    /// `(uploads_removed, temp_files_removed)`. Best-effort — errors on
    /// individual entries are logged and skipped, not propagated.
    pub async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64) {
        let now = chrono::Utc::now();
        let mut uploads_removed = 0u64;
        let mut temp_removed = 0u64;

        let mut bucket_entries = match fs::read_dir(&self.buckets_dir).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("housekeeping: cannot read buckets dir: {}", e);
                return (0, 0);
            }
        };

        while let Ok(Some(bucket_entry)) = bucket_entries.next_entry().await {
            if !matches!(bucket_entry.file_type().await, Ok(ft) if ft.is_dir()) {
                continue;
            }
            let bucket_dir = bucket_entry.path();

            // 1. Stale multipart uploads.
            let uploads_dir = bucket_dir.join(".uploads");
            if let Ok(mut uploads) = fs::read_dir(&uploads_dir).await {
                while let Ok(Some(up)) = uploads.next_entry().await {
                    if !matches!(up.file_type().await, Ok(ft) if ft.is_dir()) {
                        continue;
                    }
                    let meta_path = up.path().join(".meta.json");
                    let initiated = fs::read_to_string(&meta_path)
                        .await
                        .ok()
                        .and_then(|d| serde_json::from_str::<MultipartUploadMeta>(&d).ok())
                        .map(|m| m.initiated);
                    // Treat unreadable/missing meta as stale by directory mtime.
                    let age_ok = match initiated {
                        Some(ts) => chrono::DateTime::parse_from_rfc3339(&ts)
                            .map(|t| now.signed_duration_since(t.with_timezone(&chrono::Utc)))
                            .map(|age| age > stale_after)
                            .unwrap_or(true),
                        None => true,
                    };
                    if age_ok {
                        match fs::remove_dir_all(up.path()).await {
                            Ok(()) => {
                                uploads_removed += 1;
                                tracing::info!(
                                    "housekeeping: aborted stale multipart upload {}",
                                    up.file_name().to_string_lossy()
                                );
                            }
                            Err(e) => tracing::warn!(
                                "housekeeping: failed to remove stale upload {}: {}",
                                up.path().display(),
                                e
                            ),
                        }
                    }
                }
            }

            // 2. Leftover temp files from crashed writes (bucket root level).
            temp_removed += Self::sweep_temp_files(&bucket_dir).await;
            temp_removed += Self::sweep_temp_files(&uploads_dir).await;
        }

        (uploads_removed, temp_removed)
    }

    /// Remove `.maxio-tmp-*` entries directly inside `dir`.
    async fn sweep_temp_files(dir: &Path) -> u64 {
        let mut removed = 0u64;
        let mut entries = match fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => return 0,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(".maxio-tmp-") {
                continue;
            }
            let path = entry.path();
            let result = match entry.file_type().await {
                Ok(ft) if ft.is_dir() => fs::remove_dir_all(&path).await,
                _ => fs::remove_file(&path).await,
            };
            match result {
                Ok(()) => {
                    removed += 1;
                    tracing::info!("housekeeping: removed leftover temp {}", path.display());
                }
                Err(e) => {
                    tracing::warn!(
                        "housekeeping: failed to remove temp {}: {}",
                        path.display(),
                        e
                    )
                }
            }
        }
        removed
    }

    // --- Internal helpers ---

    async fn read_object_meta(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        let meta_path = self.meta_path(bucket, key);
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    fn walk_dir<'a>(
        &'a self,
        base: &'a Path,
        dir: &'a Path,
        prefix: &'a str,
        results: &'a mut Vec<ObjectMeta>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = match fs::read_dir(dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let fname = entry.file_name().to_string_lossy().to_string();

                if fname.ends_with(".meta.json")
                    || fname == ".bucket.json"
                    || fname == ".uploads"
                    || fname == ".versions"
                    || fname == ".folder"
                {
                    continue;
                }

                // EC chunk directory: derive the object key and read its metadata
                if fname.ends_with(".ec") && entry.file_type().await?.is_dir() {
                    if let Ok(rel) = path.strip_prefix(base) {
                        let rel_str = rel.to_string_lossy();
                        // Strip the .ec suffix to get the key
                        let key = rel_str.strip_suffix(".ec").unwrap_or(&rel_str).to_string();
                        if key.starts_with(prefix) {
                            if let Ok(meta) = self
                                .read_object_meta(base.file_name().unwrap().to_str().unwrap(), &key)
                                .await
                            {
                                results.push(meta);
                            }
                        }
                    }
                    continue;
                }

                if entry.file_type().await?.is_dir() {
                    // Check for folder marker inside this directory
                    let marker = path.join(".folder.meta.json");
                    if marker.exists() {
                        if let Ok(rel) = path.strip_prefix(base) {
                            let key = format!("{}/", rel.to_string_lossy());
                            if key.starts_with(prefix) {
                                if let Ok(data) = fs::read_to_string(&marker).await {
                                    if let Ok(meta) = serde_json::from_str::<ObjectMeta>(&data) {
                                        results.push(meta);
                                    }
                                }
                            }
                        }
                    }
                    self.walk_dir(base, &path, prefix, results).await?;
                } else {
                    if let Ok(rel) = path.strip_prefix(base) {
                        let key = rel.to_string_lossy().to_string();
                        if key.starts_with(prefix) {
                            if let Ok(meta) = self
                                .read_object_meta(base.file_name().unwrap().to_str().unwrap(), &key)
                                .await
                            {
                                results.push(meta);
                            }
                        }
                    }
                }
            }
            Ok(())
        })
    }

    async fn read_upload_meta(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<MultipartUploadMeta, StorageError> {
        let path = self.upload_meta_path(bucket, upload_id);
        let data = fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::UploadNotFound(upload_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    async fn read_part_meta(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
    ) -> Result<PartMeta, StorageError> {
        let path = self.part_meta_path(bucket, upload_id, part_number);
        let data = fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::InvalidKey(format!("missing part {}", part_number))
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    // --- Versioning ---

    fn generate_version_id() -> String {
        let micros = chrono::Utc::now().timestamp_micros() as u64;
        let rand_suffix: u32 = rand::rng().random();
        format!("{:016}-{:08x}", micros, rand_suffix)
    }

    /// Directory holding versions for a given key.
    /// For key `photos/vacation.jpg` → `{bucket}/photos/.versions/vacation.jpg/`
    fn versions_dir(&self, bucket: &str, key: &str) -> PathBuf {
        let key_path = Path::new(key);
        let parent = key_path.parent().unwrap_or(Path::new(""));
        let name = key_path.file_name().unwrap_or(std::ffi::OsStr::new(key));
        self.buckets_dir
            .join(bucket)
            .join(parent)
            .join(".versions")
            .join(name)
    }

    fn version_data_path(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket, key)
            .join(format!("{}.data", version_id))
    }

    fn version_meta_path(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket, key)
            .join(format!("{}.meta.json", version_id))
    }

    pub async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: BucketMeta = serde_json::from_str(&data)?;
        Ok(meta.versioning)
    }

    pub async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        let was_enabled = meta.versioning;
        meta.versioning = enabled;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;

        // S3-compatible suspension preserves historical versions. It only
        // changes how future writes/deletes are versioned.
        let _ = was_enabled;
        Ok(())
    }

    /// Returns bucket policy JSON and ACL for anonymous access checks.
    pub async fn get_bucket_auth_info(
        &self,
        bucket: &str,
    ) -> Result<(Option<String>, Option<crate::iam::Acl>), StorageError> {
        let meta = self.load_bucket_meta(bucket).await?;
        Ok((meta.policy.clone(), meta.acl.clone()))
    }

    pub async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<crate::storage::CorsRule>,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.cors_rules = Some(rules);
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_cors(
        &self,
        bucket: &str,
    ) -> Result<Option<Vec<crate::storage::CorsRule>>, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: BucketMeta = serde_json::from_str(&data)?;
        Ok(meta.cors_rules)
    }

    pub async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.cors_rules = None;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }


    /// Write a new version to the `.versions/` directory and update the current (top-level) files.
    async fn write_version(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
        data_path: &Path,
    ) -> Result<(), StorageError> {
        let version_id = meta.version_id.as_ref().unwrap();
        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;

        // Copy data to version store
        let ver_data = ver_dir.join(format!("{}.data", version_id));
        fs::copy(data_path, &ver_data).await?;

        // Write version metadata
        let ver_meta = ver_dir.join(format!("{}.meta.json", version_id));
        fs::write(&ver_meta, serde_json::to_string_pretty(meta)?).await?;

        Ok(())
    }

    /// Write a new chunked version: copy .ec/ dir to .versions/{key}/{version_id}.ec/
    async fn write_version_chunked(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
    ) -> Result<(), StorageError> {
        let version_id = meta.version_id.as_ref().unwrap();
        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;

        // Copy the entire .ec/ directory
        let src_ec = self.ec_dir(bucket, key);
        let dst_ec = ver_dir.join(format!("{}.ec", version_id));
        fs::create_dir_all(&dst_ec).await?;
        let mut entries = fs::read_dir(&src_ec).await?;
        while let Some(entry) = entries.next_entry().await? {
            let dest = dst_ec.join(entry.file_name());
            fs::copy(entry.path(), &dest).await?;
        }

        // Write version metadata
        let ver_meta = ver_dir.join(format!("{}.meta.json", version_id));
        fs::write(&ver_meta, serde_json::to_string_pretty(meta)?).await?;

        Ok(())
    }

    /// Write a delete marker version and remove the top-level files.
    async fn write_delete_marker(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<DeleteResult, StorageError> {
        let version_id = Self::generate_version_id();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let marker_meta = ObjectMeta {
            key: key.to_string(),
            size: 0,
            etag: String::new(),
            content_type: String::new(),
            last_modified: now,
            owner_id: String::new(),
            owner_display_name: String::new(),
            acl: None,
            version_id: Some(version_id.clone()),
            is_delete_marker: true,
            storage_format: None,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
        };

        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;
        let ver_meta_path = ver_dir.join(format!("{}.meta.json", version_id));
        fs::write(&ver_meta_path, serde_json::to_string_pretty(&marker_meta)?).await?;

        // Remove top-level current files
        let _ = fs::remove_file(self.object_path(bucket, key)).await;
        let _ = fs::remove_file(self.meta_path(bucket, key)).await;
        let _ = fs::remove_dir_all(self.ec_dir(bucket, key)).await;

        Ok(DeleteResult {
            version_id: Some(version_id),
            is_delete_marker: true,
        })
    }

    /// Scan versions for a key and update the top-level files to reflect the latest non-delete-marker.
    async fn update_current_version(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let ver_dir = self.versions_dir(bucket, key);
        if !fs::try_exists(&ver_dir).await.unwrap_or(false) {
            return Ok(());
        }

        // Find the latest non-delete-marker version (lexicographic sort = chronological)
        let mut versions = Vec::new();
        let mut entries = fs::read_dir(&ver_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".meta.json") {
                versions.push(fname);
            }
        }
        versions.sort();
        versions.reverse(); // newest first

        for meta_fname in &versions {
            let meta_path = ver_dir.join(meta_fname);
            let data = fs::read_to_string(&meta_path).await?;
            let meta: ObjectMeta = serde_json::from_str(&data)?;
            if !meta.is_delete_marker {
                // Restore this version as current
                let vid = meta.version_id.as_ref().unwrap();
                let obj_meta_path = self.meta_path(bucket, key);

                let ver_ec = ver_dir.join(format!("{}.ec", vid));
                if ver_ec.is_dir() {
                    // Restore chunked version
                    let dst_ec = self.ec_dir(bucket, key);
                    if let Some(parent) = dst_ec.parent() {
                        fs::create_dir_all(parent).await?;
                    }
                    let _ = fs::remove_dir_all(&dst_ec).await;
                    fs::create_dir_all(&dst_ec).await?;
                    let mut entries = fs::read_dir(&ver_ec).await?;
                    while let Some(entry) = entries.next_entry().await? {
                        fs::copy(entry.path(), dst_ec.join(entry.file_name())).await?;
                    }
                } else {
                    // Restore flat version
                    let ver_data = ver_dir.join(format!("{}.data", vid));
                    let obj_path = self.object_path(bucket, key);
                    if let Some(parent) = obj_path.parent() {
                        fs::create_dir_all(parent).await?;
                    }
                    fs::copy(&ver_data, &obj_path).await?;
                }

                if let Some(parent) = obj_meta_path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::write(&obj_meta_path, serde_json::to_string_pretty(&meta)?).await?;
                return Ok(());
            }
        }

        // All versions are delete markers — remove top-level files
        let _ = fs::remove_file(self.object_path(bucket, key)).await;
        let _ = fs::remove_file(self.meta_path(bucket, key)).await;
        let _ = fs::remove_dir_all(self.ec_dir(bucket, key)).await;
        Ok(())
    }

    pub async fn get_object_version(
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
        let ver_meta_path = self.version_meta_path(bucket, key, version_id);
        let data = fs::read_to_string(&ver_meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: ObjectMeta = serde_json::from_str(&data)?;

        if meta.is_delete_marker {
            return Err(StorageError::NotFound(key.to_string()));
        }

        let ver_ec_dir = self
            .versions_dir(bucket, key)
            .join(format!("{}.ec", version_id));
        if ver_ec_dir.is_dir() {
            let manifest_path = ver_ec_dir.join("manifest.json");
            let manifest_data = fs::read_to_string(&manifest_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::VersionNotFound(version_id.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            let manifest: ChunkManifest = serde_json::from_str(&manifest_data)?;
            let reader = VerifiedChunkReader::new(ver_ec_dir, manifest);
            return Ok((Box::pin(reader), meta));
        }

        let ver_data_path = self.version_data_path(bucket, key, version_id);
        let file = fs::File::open(&ver_data_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok((
            Box::pin(BufReader::with_capacity(IO_BUFFER_SIZE, file)),
            meta,
        ))
    }

    pub async fn head_object_version(
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
        let ver_meta_path = self.version_meta_path(bucket, key, version_id);
        let data = fs::read_to_string(&ver_meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: ObjectMeta = serde_json::from_str(&data)?;
        if meta.is_delete_marker {
            return Err(StorageError::NotFound(key.to_string()));
        }
        Ok(meta)
    }

    pub async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if version_id == "null" {
            let meta = self.read_object_meta(bucket, key).await?;
            remove_file_if_exists(&self.object_path(bucket, key)).await?;
            remove_file_if_exists(&self.meta_path(bucket, key)).await?;
            remove_dir_all_if_exists(&self.ec_dir(bucket, key)).await?;
            self.update_current_version(bucket, key).await?;
            return Ok(meta);
        }
        let ver_meta_path = self.version_meta_path(bucket, key, version_id);
        let data = fs::read_to_string(&ver_meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: ObjectMeta = serde_json::from_str(&data)?;

        // Remove version files
        let _ = fs::remove_file(&ver_meta_path).await;
        let ver_data_path = self.version_data_path(bucket, key, version_id);
        let _ = fs::remove_file(&ver_data_path).await;
        let ver_ec_dir = self
            .versions_dir(bucket, key)
            .join(format!("{}.ec", version_id));
        let _ = fs::remove_dir_all(&ver_ec_dir).await;

        // Clean up empty versions dir
        let ver_dir = self.versions_dir(bucket, key);
        let _ = fs::remove_dir(&ver_dir).await; // only succeeds if empty

        // Update current version (in case we deleted the latest or a delete marker)
        self.update_current_version(bucket, key).await?;

        Ok(meta)
    }

    pub async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        let bucket_dir = self.buckets_dir.join(bucket);
        let mut results = Vec::new();
        self.walk_versions(&bucket_dir, &bucket_dir, prefix, &mut results)
            .await?;
        // S3's suspended state can have a current "null" version outside
        // .versions. Include it in version listings when present.
        for obj in self.list_objects(bucket, prefix).await? {
            if obj.version_id.is_none() {
                results.push(obj);
            }
        }
        // Sort by key, then by version_id descending (newest first per key)
        results.sort_by(|a, b| {
            a.key.cmp(&b.key).then_with(|| {
                let va = a.version_id.as_deref().unwrap_or("");
                let vb = b.version_id.as_deref().unwrap_or("");
                vb.cmp(va)
            })
        });
        Ok(results)
    }

    fn walk_versions<'a>(
        &'a self,
        base: &'a Path,
        dir: &'a Path,
        prefix: &'a str,
        results: &'a mut Vec<ObjectMeta>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = match fs::read_dir(dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let fname = entry.file_name().to_string_lossy().to_string();

                if !entry.file_type().await?.is_dir() {
                    continue;
                }

                if fname == ".versions" {
                    // Scan all key dirs inside .versions
                    let mut key_dirs = match fs::read_dir(&path).await {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    while let Some(key_entry) = key_dirs.next_entry().await? {
                        if !key_entry.file_type().await?.is_dir() {
                            continue;
                        }
                        let key_name = key_entry.file_name().to_string_lossy().to_string();
                        // Reconstruct the object key from the directory structure
                        let parent_rel = dir.strip_prefix(base).unwrap_or(Path::new(""));
                        let key = if parent_rel.as_os_str().is_empty() {
                            key_name.clone()
                        } else {
                            format!("{}/{}", parent_rel.to_string_lossy(), key_name)
                        };
                        if !key.starts_with(prefix) {
                            continue;
                        }
                        // Read all version meta files in this key's version dir
                        let key_ver_dir = key_entry.path();
                        let mut ver_entries = match fs::read_dir(&key_ver_dir).await {
                            Ok(e) => e,
                            Err(_) => continue,
                        };
                        while let Some(ve) = ver_entries.next_entry().await? {
                            let vf = ve.file_name().to_string_lossy().to_string();
                            if vf.ends_with(".meta.json") {
                                if let Ok(data) = fs::read_to_string(ve.path()).await {
                                    if let Ok(meta) = serde_json::from_str::<ObjectMeta>(&data) {
                                        results.push(meta);
                                    }
                                }
                            }
                        }
                    }
                } else if fname != ".uploads" && fname != ".bucket.json" {
                    self.walk_versions(base, &path, prefix, results).await?;
                }
            }
            Ok(())
        })
    }
}


async fn remove_file_if_exists(path: &Path) -> Result<(), StorageError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StorageError::Io(e)),
    }
}

async fn remove_dir_all_if_exists(path: &Path) -> Result<(), StorageError> {
    match fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StorageError::Io(e)),
    }
}
