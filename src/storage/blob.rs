use base64::Engine;

use super::cache::CacheLayer;
use super::hashing::{ChecksumHasher, EtagMd5};
use super::{ByteStream, ChecksumAlgorithm, ObjectMeta, PartMeta, StorageError};
use crate::metrics::MetricsRegistry;
use rand::RngExt;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter};

pub const IO_BUFFER_SIZE: usize = 256 * 1024;
pub(crate) const SMALL_OBJECT_THRESHOLD: u64 = 256 * 1024;
const SMALL_WRITE_READ_CHUNK: usize = 8192;

pub struct BlobStorage {
    pub(crate) buckets_dir: PathBuf,
    cache: Option<Arc<CacheLayer>>,
    /// Directories known to already exist — avoids a `create_dir_all` syscall on repeat paths.
    known_dirs: Mutex<HashSet<PathBuf>>,
    metrics: Option<Arc<MetricsRegistry>>,
}

pub struct WrittenPayload {
    pub size: u64,
    pub etag: String,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub checksum_value: Option<String>,
    pub tmp_path: PathBuf,
    pub final_path: PathBuf,
}

pub fn validate_key(key: &str) -> Result<(), StorageError> {
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

fn is_reserved_segment(name: &str) -> bool {
    name.ends_with(".meta.json")
        || name == ".bucket.json"
        || name == ".uploads"
        || name == ".versions"
        || name == ".folder"
        || name.starts_with(".maxio-tmp-")
}

pub fn validate_upload_id(upload_id: &str) -> Result<(), StorageError> {
    if upload_id.is_empty() {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }
    if upload_id.contains('/') || upload_id.contains('\\') || upload_id.contains("..") {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }
    Ok(())
}

pub(crate) fn object_path_in(buckets_dir: &Path, bucket: &str, key: &str) -> PathBuf {
    if key.ends_with('/') {
        let dir = key.trim_end_matches('/');
        buckets_dir.join(bucket).join(dir).join(".folder")
    } else {
        buckets_dir.join(bucket).join(key)
    }
}

impl BlobStorage {
    pub async fn new(data_dir: &str) -> Result<Self, anyhow::Error> {
        let buckets_dir = Path::new(data_dir).join("buckets");
        fs::create_dir_all(&buckets_dir).await?;
        Ok(Self {
            buckets_dir,
            cache: None,
            known_dirs: Mutex::new(HashSet::new()),
            metrics: None,
        })
    }

    pub fn with_metrics(mut self, metrics: Arc<MetricsRegistry>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn with_cache(mut self, cache: Arc<CacheLayer>) -> Self {
        self.cache = Some(cache);
        self
    }

    #[inline]
    fn record_drive_read(&self) {
        if let Some(ref m) = self.metrics {
            m.record_drive_read_op();
        }
    }

    #[inline]
    fn record_drive_write(&self) {
        if let Some(ref m) = self.metrics {
            m.record_drive_write_op();
        }
    }

    fn staging_buckets_dir(&self) -> &Path {
        self.cache
            .as_ref()
            .map(|c| c.buckets_dir())
            .unwrap_or(&self.buckets_dir)
    }

    fn write_buckets_dir(&self) -> Result<&Path, StorageError> {
        if let Some(cache) = &self.cache {
            if cache.writeback() {
                return Ok(cache.buckets_dir());
            }
        }
        Ok(&self.buckets_dir)
    }

    pub async fn complete_object_write(
        &self,
        bucket: &str,
        key: &str,
        final_path: &Path,
        size: u64,
    ) -> Result<(), StorageError> {
        let Some(cache) = &self.cache else {
            return Ok(());
        };
        if cache.writeback() {
            cache.mark_dirty(bucket, key, size).await;
            return Ok(());
        }
        if final_path.starts_with(&self.buckets_dir) {
            cache
                .populate_from_data(bucket, key, final_path, size)
                .await?;
        }
        Ok(())
    }

    async fn resolve_read_path(
        &self,
        bucket: &str,
        key: &str,
        expected_size: u64,
    ) -> Result<PathBuf, StorageError> {
        if let Some(cache) = &self.cache {
            let cache_path = cache.object_path(bucket, key);
            match fs::metadata(&cache_path).await {
                Ok(m) if m.len() == expected_size => {
                    cache.record_read_hit(bucket, key, expected_size).await;
                    return Ok(cache_path);
                }
                Ok(_) => {
                    // Partial/stale cache file from an interrupted write — drop and fall back.
                    let _ = fs::remove_file(&cache_path).await;
                    cache.remove_entry(bucket, key).await;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    cache.remove_entry(bucket, key).await;
                }
                Err(_) => {}
            }
        }

        let data_path = self.object_path(bucket, key);
        match fs::metadata(&data_path).await {
            Ok(m) if m.len() == expected_size => {}
            Ok(_) => return Err(StorageError::NotFound(key.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound(key.to_string()));
            }
            Err(e) => return Err(StorageError::Io(e)),
        }

        if let Some(cache) = &self.cache {
            return cache
                .populate_from_data(bucket, key, &data_path, expected_size)
                .await;
        }

        Ok(data_path)
    }

    pub fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        object_path_in(&self.buckets_dir, bucket, key)
    }

    pub fn uploads_dir(&self, bucket: &str) -> PathBuf {
        self.staging_buckets_dir().join(bucket).join(".uploads")
    }

    pub fn upload_dir(&self, bucket: &str, upload_id: &str) -> PathBuf {
        self.uploads_dir(bucket).join(upload_id)
    }

    pub fn part_path(&self, bucket: &str, upload_id: &str, part_number: u32) -> PathBuf {
        self.upload_dir(bucket, upload_id)
            .join(part_number.to_string())
    }

    pub fn versions_dir(&self, bucket: &str, key: &str) -> PathBuf {
        let key_path = Path::new(key);
        let parent = key_path.parent().unwrap_or(Path::new(""));
        let name = key_path.file_name().unwrap_or(std::ffi::OsStr::new(key));
        self.buckets_dir
            .join(bucket)
            .join(parent)
            .join(".versions")
            .join(name)
    }

    pub fn version_data_path(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket, key)
            .join(format!("{}.data", version_id))
    }

    pub async fn ensure_upload_dir(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError> {
        fs::create_dir_all(self.upload_dir(bucket, upload_id)).await?;
        Ok(())
    }

    pub async fn write_folder_marker(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let folder_dir = self
            .buckets_dir
            .join(bucket)
            .join(key.trim_end_matches('/'));
        fs::create_dir_all(&folder_dir).await?;
        fs::write(folder_dir.join(".folder"), b"").await?;
        Ok(())
    }

    async fn ensure_object_parent_dir(&self, obj_path: &Path) -> Result<(), StorageError> {
        let parent = obj_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let needs_create = {
            let mut dirs = self.known_dirs.lock().unwrap();
            if dirs.contains(&parent) {
                false
            } else {
                dirs.insert(parent.clone());
                true
            }
        };
        if needs_create {
            fs::create_dir_all(&parent).await?;
        }
        Ok(())
    }

    pub async fn write_flat_object_temp(
        &self,
        bucket: &str,
        key: &str,
        mut body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        content_length: Option<u64>,
    ) -> Result<WrittenPayload, StorageError> {
        let write_base = self.write_buckets_dir()?;
        let obj_path = object_path_in(write_base, bucket, key);
        self.ensure_object_parent_dir(&obj_path).await?;

        if let Some(size) = content_length {
            if size > SMALL_OBJECT_THRESHOLD {
                return self
                    .write_flat_object_temp_streaming(obj_path, Vec::new(), body, checksum)
                    .await;
            }
            let mut data = Vec::with_capacity(size as usize);
            body.read_to_end(&mut data)
                .await
                .map_err(StorageError::Io)?;
            return self
                .write_flat_object_temp_buffered(obj_path, data, checksum)
                .await;
        }

        let mut chunk = [0u8; SMALL_WRITE_READ_CHUNK];
        let mut prefix = Vec::with_capacity(SMALL_WRITE_READ_CHUNK);
        loop {
            let n = body.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            prefix.extend_from_slice(&chunk[..n]);
            if prefix.len() as u64 > SMALL_OBJECT_THRESHOLD {
                return self
                    .write_flat_object_temp_streaming(obj_path, prefix, body, checksum)
                    .await;
            }
        }

        self.write_flat_object_temp_buffered(obj_path, prefix, checksum)
            .await
    }

    async fn write_flat_object_temp_buffered(
        &self,
        obj_path: PathBuf,
        data: Vec<u8>,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<WrittenPayload, StorageError> {
        let size = data.len() as u64;
        let (etag, checksum_algorithm, checksum_value) = digest_flat_write(&data, checksum)?;

        let write_path = temp_sibling_path(&obj_path);
        let mut tmp_guard = TempPathGuard::new(write_path.clone());
        fs::write(&write_path, &data)
            .await
            .map_err(StorageError::Io)?;

        tmp_guard.disarm();
        self.record_drive_write();

        Ok(WrittenPayload {
            size,
            etag,
            checksum_algorithm,
            checksum_value,
            tmp_path: write_path,
            final_path: obj_path,
        })
    }

    async fn write_flat_object_temp_streaming(
        &self,
        obj_path: PathBuf,
        mut prefix: Vec<u8>,
        mut body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<WrittenPayload, StorageError> {
        let write_path = temp_sibling_path(&obj_path);
        let mut tmp_guard = TempPathGuard::new(write_path.clone());

        let file = fs::File::create(&write_path).await?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, file);
        let mut hasher = EtagMd5::new();
        let mut checksum_hasher = checksum
            .as_ref()
            .map(|(algo, _)| ChecksumHasher::new(*algo));
        let mut size: u64 = 0;
        let mut buf = vec![0u8; IO_BUFFER_SIZE];

        let mut update = |slice: &[u8]| {
            hasher.update(slice);
            if let Some(ref mut ch) = checksum_hasher {
                ch.update(slice);
            }
            size += slice.len() as u64;
        };

        update(&prefix);
        writer.write_all(&prefix).await?;
        prefix.clear();

        loop {
            let n = body.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            update(&buf[..n]);
            writer.write_all(&buf[..n]).await?;
        }
        writer.flush().await?;

        let (etag, checksum_algorithm, checksum_value) =
            finalize_flat_write_hashes(hasher, checksum_hasher, checksum)?;

        tmp_guard.disarm();
        self.record_drive_write();

        Ok(WrittenPayload {
            size,
            etag,
            checksum_algorithm,
            checksum_value,
            tmp_path: write_path,
            final_path: obj_path,
        })
    }

    pub async fn publish_temp_payload(
        tmp_payload: &Path,
        final_payload: &Path,
    ) -> Result<(), StorageError> {
        // POSIX rename is atomic: atomically replaces any existing file at final_payload.
        fs::rename(tmp_payload, final_payload)
            .await
            .map_err(StorageError::Io)
    }

    pub async fn remove_published_payload(path: &Path) -> Result<(), StorageError> {
        remove_file_if_exists(path).await
    }

    pub async fn open_object(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
    ) -> Result<ByteStream, StorageError> {
        let obj_path = self.resolve_read_path(bucket, key, meta.size).await?;
        self.record_drive_read();
        if meta.size <= SMALL_OBJECT_THRESHOLD {
            let data = fs::read(&obj_path).await.map_err(StorageError::Io)?;
            return Ok(Box::pin(std::io::Cursor::new(data)));
        }
        let file = fs::File::open(&obj_path).await.map_err(StorageError::Io)?;
        Ok(Box::pin(BufReader::with_capacity(IO_BUFFER_SIZE, file)))
    }

    pub async fn open_object_range(
        &self,
        bucket: &str,
        key: &str,
        _meta: &ObjectMeta,
        offset: u64,
        length: u64,
    ) -> Result<ByteStream, StorageError> {
        let obj_path = self.resolve_read_path(bucket, key, _meta.size).await?;
        self.record_drive_read();
        if length <= SMALL_OBJECT_THRESHOLD {
            let mut file = fs::File::open(&obj_path).await.map_err(StorageError::Io)?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(StorageError::Io)?;
            let mut data = vec![0u8; length as usize];
            file.read_exact(&mut data).await.map_err(StorageError::Io)?;
            return Ok(Box::pin(std::io::Cursor::new(data)));
        }
        let mut file = fs::File::open(&obj_path).await.map_err(StorageError::Io)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(StorageError::Io)?;
        let limited = file.take(length);
        Ok(Box::pin(BufReader::with_capacity(IO_BUFFER_SIZE, limited)))
    }

    pub async fn unlink_object(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        self.unlink_object_inner(bucket, key, true).await
    }

    async fn unlink_object_inner(
        &self,
        bucket: &str,
        key: &str,
        cleanup_parents: bool,
    ) -> Result<(), StorageError> {
        let obj_path = self.object_path(bucket, key);
        let writeback = self.cache.as_ref().is_some_and(|c| c.writeback());

        if let Some(cache) = &self.cache {
            cache.remove_entry(bucket, key).await;
            let cache_path = cache.object_path(bucket, key);
            let _ = remove_file_if_exists(&cache_path).await;
        }

        if writeback {
            self.record_drive_write();
            let bucket_dir = self.buckets_dir.join(bucket);
            tokio::spawn(async move {
                let _ = remove_file_if_exists(&obj_path).await;
                if cleanup_parents {
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
                }
            });
            return Ok(());
        }

        remove_file_if_exists(&obj_path).await?;
        self.record_drive_write();
        if cleanup_parents {
            self.cleanup_empty_parents(bucket, key).await;
        }
        Ok(())
    }

    /// Unlink many object files with bounded concurrency; parent cleanup runs once after.
    pub async fn unlink_objects_batch(
        &self,
        bucket: &str,
        keys: &[String],
        concurrency: usize,
    ) -> Result<(), StorageError> {
        if keys.is_empty() {
            return Ok(());
        }
        use std::sync::Arc;
        use tokio::sync::Semaphore;

        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let buckets_dir = self.buckets_dir.clone();
        let cache = self.cache.clone();
        let writeback = cache.as_ref().is_some_and(|c| c.writeback());
        let bucket_name = bucket.to_string();
        let mut handles = Vec::with_capacity(keys.len());
        for key in keys {
            self.record_drive_write();
            let permit = sem
                .clone()
                .acquire_owned()
                .await
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
            let buckets_dir = buckets_dir.clone();
            let cache = cache.clone();
            let bucket_name = bucket_name.clone();
            let key = key.clone();
            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let obj_path = object_path_in(&buckets_dir, &bucket_name, &key);
                if let Some(cache) = &cache {
                    cache.remove_entry(&bucket_name, &key).await;
                    let cache_path = cache.object_path(&bucket_name, &key);
                    let _ = remove_file_if_exists(&cache_path).await;
                }
                if writeback {
                    tokio::spawn(async move {
                        let _ = remove_file_if_exists(&obj_path).await;
                    });
                } else {
                    let _ = remove_file_if_exists(&obj_path).await;
                }
            }));
        }
        for handle in handles {
            handle
                .await
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
        }
        if writeback {
            let buckets_dir = self.buckets_dir.clone();
            let bucket_name = bucket.to_string();
            tokio::spawn(async move {
                prune_empty_subdirs(&buckets_dir.join(&bucket_name)).await;
            });
        } else {
            self.prune_empty_dirs(bucket).await;
        }
        Ok(())
    }

    pub async fn remove_bucket_tree(&self, bucket: &str) -> Result<(), StorageError> {
        let writeback = self.cache.as_ref().is_some_and(|c| c.writeback());
        if let Some(cache) = &self.cache {
            cache.purge_bucket(bucket).await;
            let cache_path = cache.buckets_dir().join(bucket);
            let _ = fs::remove_dir_all(&cache_path).await;
        }
        let path = self.buckets_dir.join(bucket);
        if writeback {
            tokio::spawn(async move {
                let _ = remove_dir_all_if_exists(&path).await;
            });
            return Ok(());
        }
        match fs::remove_dir_all(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(StorageError::Io(e)),
        }
        Ok(())
    }

    pub async fn prune_empty_dirs(&self, bucket: &str) {
        prune_empty_subdirs(&self.buckets_dir.join(bucket)).await;
    }

    pub async fn cleanup_empty_parents(&self, bucket: &str, key: &str) {
        let obj_path = self.object_path(bucket, key);
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
    }

    pub async fn write_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        mut body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<(String, u64, Option<ChecksumAlgorithm>, Option<String>), StorageError> {
        self.ensure_upload_dir(bucket, upload_id).await?;
        let part_path = self.part_path(bucket, upload_id, part_number);
        let file = fs::File::create(&part_path).await?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, file);
        let mut hasher = EtagMd5::new();
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
        self.record_drive_write();
        Ok((etag, size, checksum_algorithm, checksum_value))
    }

    pub async fn remove_part_file(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
    ) -> Result<(), StorageError> {
        remove_file_if_exists(&self.part_path(bucket, upload_id, part_number)).await
    }

    pub async fn assemble_multipart_temp(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[PartMeta],
    ) -> Result<WrittenPayload, StorageError> {
        let write_base = self.write_buckets_dir()?;
        let obj_path = object_path_in(write_base, bucket, key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_obj_path = temp_sibling_path(&obj_path);
        let mut tmp_obj_guard = TempPathGuard::new(tmp_obj_path.clone());

        let (total_size, etag) = if parts.len() == 1 {
            let part = &parts[0];
            let part_path = self.part_path(bucket, upload_id, part.part_number);
            fs::rename(&part_path, &tmp_obj_path)
                .await
                .map_err(StorageError::Io)?;
            let raw_md5 = hex::decode(part.etag.trim_matches('"'))
                .map_err(|_| StorageError::InvalidKey("invalid part etag".into()))?;
            let mut etag_hasher = EtagMd5::new();
            etag_hasher.update(&raw_md5);
            let etag = format!("\"{}-1\"", hex::encode(etag_hasher.finalize()));
            (part.size, etag)
        } else {
            let out = fs::File::create(&tmp_obj_path).await?;
            let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, out);
            let mut total_size = 0u64;
            let mut etag_hasher = EtagMd5::new();
            let mut buf = vec![0u8; IO_BUFFER_SIZE];

            for part in parts {
                let part_path = self.part_path(bucket, upload_id, part.part_number);
                let part_file = fs::File::open(&part_path).await?;
                let mut part_stream = BufReader::with_capacity(IO_BUFFER_SIZE, part_file);
                total_size += stream_to_writer(&mut part_stream, &mut writer, &mut buf).await?;

                let raw_md5 = hex::decode(part.etag.trim_matches('"'))
                    .map_err(|_| StorageError::InvalidKey("invalid part etag".into()))?;
                etag_hasher.update(&raw_md5);
            }
            writer.flush().await?;
            let etag = format!(
                "\"{}-{}\"",
                hex::encode(etag_hasher.finalize()),
                parts.len()
            );
            (total_size, etag)
        };

        tmp_obj_guard.disarm();
        self.record_drive_write();
        Ok(WrittenPayload {
            size: total_size,
            etag,
            checksum_algorithm: None,
            checksum_value: None,
            tmp_path: tmp_obj_path,
            final_path: obj_path,
        })
    }

    pub fn composite_multipart_checksum(
        algo: ChecksumAlgorithm,
        parts: &[PartMeta],
    ) -> Option<String> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut raw_checksums = Vec::new();
        for part in parts {
            if let Some(ref val) = part.checksum_value {
                if let Ok(raw) = b64.decode(val) {
                    raw_checksums.extend_from_slice(&raw);
                }
            }
        }
        if raw_checksums.is_empty() {
            return None;
        }
        let mut composite_hasher = ChecksumHasher::new(algo);
        composite_hasher.update(&raw_checksums);
        Some(format!(
            "{}-{}",
            composite_hasher.finalize_base64(),
            parts.len()
        ))
    }

    pub async fn remove_upload_dir(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError> {
        remove_dir_all_if_exists(&self.upload_dir(bucket, upload_id)).await
    }

    pub fn generate_version_id() -> String {
        let micros = chrono::Utc::now().timestamp_micros() as u64;
        let rand_suffix: u32 = rand::rng().random();
        format!("{:016}-{:08x}", micros, rand_suffix)
    }

    pub async fn archive_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        data_path: &Path,
    ) -> Result<(), StorageError> {
        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;
        fs::copy(data_path, self.version_data_path(bucket, key, version_id)).await?;
        self.record_drive_write();
        Ok(())
    }

    pub async fn unlink_version_blobs(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        let _ = fs::remove_file(self.version_data_path(bucket, key, version_id)).await;
        let ver_dir = self.versions_dir(bucket, key);
        let _ = fs::remove_dir(&ver_dir).await;
        Ok(())
    }

    /// Unlink many version blob files with bounded concurrency.
    pub async fn unlink_version_blobs_batch(
        &self,
        bucket: &str,
        pairs: &[(String, String)],
        concurrency: usize,
    ) -> Result<(), StorageError> {
        if pairs.is_empty() {
            return Ok(());
        }
        use std::sync::Arc;
        use tokio::sync::Semaphore;

        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let mut handles = Vec::with_capacity(pairs.len());
        for (key, version_id) in pairs {
            let permit = sem
                .clone()
                .acquire_owned()
                .await
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
            let ver_data = self.version_data_path(bucket, key, version_id);
            let ver_dir = self.versions_dir(bucket, key);
            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let _ = fs::remove_file(ver_data).await;
                let _ = fs::remove_dir(ver_dir).await;
            }));
        }
        for handle in handles {
            handle
                .await
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
        }
        Ok(())
    }

    pub async fn open_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ByteStream, StorageError> {
        let ver_data_path = self.version_data_path(bucket, key, version_id);
        let file = fs::File::open(&ver_data_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(Box::pin(BufReader::with_capacity(IO_BUFFER_SIZE, file)))
    }

    pub async fn restore_current_from_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        let ver_data = self.version_data_path(bucket, key, version_id);
        let obj_path = self.object_path(bucket, key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(&ver_data, &obj_path).await?;
        if let Some(cache) = &self.cache {
            let size = fs::metadata(&obj_path)
                .await
                .map_err(StorageError::Io)?
                .len();
            let _ = cache.populate_from_data(bucket, key, &obj_path, size).await;
        }
        Ok(())
    }

    pub async fn housekeeping_temp_sweep(&self) -> u64 {
        let mut temp_removed = sweep_buckets_dir(&self.buckets_dir).await;
        if let Some(cache) = &self.cache {
            temp_removed += sweep_buckets_dir(cache.buckets_dir()).await;
        }
        temp_removed
    }
}

fn collect_subdirs_post_order(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            collect_subdirs_post_order(&path, out);
            out.push(path);
        }
    }
}

/// Remove every empty subdirectory under `dir` (post-order). The root `dir` itself is kept.
async fn prune_empty_subdirs(dir: &Path) {
    let mut dirs = Vec::new();
    collect_subdirs_post_order(dir, &mut dirs);
    for d in dirs {
        let _ = fs::remove_dir(&d).await;
    }
}

async fn sweep_buckets_dir(buckets_dir: &Path) -> u64 {
    let mut temp_removed = 0u64;
    let mut bucket_entries = match fs::read_dir(buckets_dir).await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("housekeeping: cannot read buckets dir: {}", e);
            return 0;
        }
    };

    while let Ok(Some(bucket_entry)) = bucket_entries.next_entry().await {
        if !matches!(bucket_entry.file_type().await, Ok(ft) if ft.is_dir()) {
            continue;
        }
        let bucket_dir = bucket_entry.path();
        temp_removed += sweep_temp_files(&bucket_dir).await;
        temp_removed += sweep_temp_files(&bucket_dir.join(".uploads")).await;
    }

    temp_removed
}

fn digest_flat_write(
    data: &[u8],
    checksum: Option<(ChecksumAlgorithm, Option<String>)>,
) -> Result<(String, Option<ChecksumAlgorithm>, Option<String>), StorageError> {
    let mut hasher = EtagMd5::new();
    hasher.update(data);
    let checksum_hasher = checksum
        .as_ref()
        .map(|(algo, _)| ChecksumHasher::new(*algo))
        .map(|mut ch| {
            ch.update(data);
            ch
        });
    finalize_flat_write_hashes(hasher, checksum_hasher, checksum)
}

fn finalize_flat_write_hashes(
    hasher: EtagMd5,
    checksum_hasher: Option<ChecksumHasher>,
    checksum: Option<(ChecksumAlgorithm, Option<String>)>,
) -> Result<(String, Option<ChecksumAlgorithm>, Option<String>), StorageError> {
    let etag = format!("\"{}\"", hex::encode(hasher.finalize()));
    let (checksum_algorithm, checksum_value) = if let Some((algo, expected)) = checksum {
        let computed = checksum_hasher.unwrap().finalize_base64();
        if let Some(expected_val) = expected {
            if computed != expected_val {
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
    Ok((etag, checksum_algorithm, checksum_value))
}

fn temp_sibling_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".maxio-tmp-{}", uuid::Uuid::new_v4()))
}

struct TempPathGuard {
    path: PathBuf,
    armed: bool,
}

impl TempPathGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Copy `reader` into `writer` using a fixed-size buffer (never the whole object).
pub async fn stream_to_writer<R, W>(
    reader: &mut R,
    writer: &mut W,
    buf: &mut [u8],
) -> Result<u64, StorageError>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut total = 0u64;
    loop {
        let n = reader.read(buf).await.map_err(StorageError::Io)?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .await
            .map_err(StorageError::Io)?;
        total += n as u64;
    }
    Ok(total)
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
