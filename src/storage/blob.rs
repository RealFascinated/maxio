use super::chunk_reader::VerifiedChunkReader;
use super::{
    ByteStream, ChecksumAlgorithm, ChunkInfo, ChunkKind, ChunkManifest, ObjectMeta, PartMeta,
    StorageError,
};
use rand::RngExt;
use base64::Engine;
use md5::{Digest, Md5};
use sha2::Sha256;
use std::path::{Component, Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter};

pub(crate) const IO_BUFFER_SIZE: usize = 256 * 1024;
pub(crate) const FLAT_WRITE_BUFFER_SIZE: usize = 8 * 1024;
pub(crate) const SMALL_OBJECT_THRESHOLD: u64 = 256 * 1024;

pub struct BlobStorage {
    pub(crate) buckets_dir: PathBuf,
    pub(crate) erasure_coding: bool,
    pub(crate) chunk_size: u64,
    pub(crate) parity_shards: u32,
}

pub struct WrittenPayload {
    pub size: u64,
    pub etag: String,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub checksum_value: Option<String>,
    pub storage_format: Option<String>,
    pub tmp_path: PathBuf,
    pub final_path: PathBuf,
    pub payload_is_dir: bool,
    /// Object bytes are already at `final_path` (no rename needed).
    pub published: bool,
}

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
        || name.ends_with(".ec")
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

impl BlobStorage {
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

    pub fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir.join(bucket).join(dir).join(".folder")
        } else {
            self.buckets_dir.join(bucket).join(key)
        }
    }

    pub fn ec_dir(&self, bucket: &str, key: &str) -> PathBuf {
        self.buckets_dir.join(bucket).join(format!("{}.ec", key))
    }

    fn manifest_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.ec_dir(bucket, key).join("manifest.json")
    }

    pub fn uploads_dir(&self, bucket: &str) -> PathBuf {
        self.buckets_dir.join(bucket).join(".uploads")
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

    pub fn version_ec_dir(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket, key)
            .join(format!("{}.ec", version_id))
    }

    pub fn erasure_coding_enabled(&self) -> bool {
        self.erasure_coding
    }

    pub async fn ensure_upload_dir(&self, bucket: &str, upload_id: &str) -> Result<(), StorageError> {
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

    pub async fn write_flat_object_temp(
        &self,
        bucket: &str,
        key: &str,
        mut body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
    ) -> Result<WrittenPayload, StorageError> {
        let obj_path = self.object_path(bucket, key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let direct_write = !fs::try_exists(&obj_path).await?;
        let write_path = if direct_write {
            obj_path.clone()
        } else {
            temp_sibling_path(&obj_path)
        };
        let tmp_obj_guard = if direct_write {
            None
        } else {
            Some(TempPathGuard::file(write_path.clone()))
        };

        let file = fs::File::create(&write_path).await?;
        let mut writer = BufWriter::with_capacity(FLAT_WRITE_BUFFER_SIZE, file);
        let mut hasher = Md5::new();
        let mut checksum_hasher = checksum
            .as_ref()
            .map(|(algo, _)| ChecksumHasher::new(*algo));
        let mut size: u64 = 0;
        let mut buf = [0u8; FLAT_WRITE_BUFFER_SIZE];

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

        let etag = format!("\"{}\"", hex::encode(hasher.finalize()));

        let (checksum_algorithm, checksum_value) = if let Some((algo, expected)) = checksum {
            let computed = checksum_hasher.unwrap().finalize_base64();
            if let Some(expected_val) = expected {
                if computed != expected_val {
                    let _ = fs::remove_file(&write_path).await;
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

        if let Some(mut guard) = tmp_obj_guard {
            guard.disarm();
        }

        Ok(WrittenPayload {
            size,
            etag,
            checksum_algorithm,
            checksum_value,
            storage_format: None,
            tmp_path: write_path,
            final_path: obj_path,
            payload_is_dir: false,
            published: direct_write,
        })
    }

    pub async fn write_chunked_object_temp(
        &self,
        bucket: &str,
        key: &str,
        mut body: ByteStream,
        checksum_algo: Option<ChecksumAlgorithm>,
    ) -> Result<WrittenPayload, StorageError> {
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

        if chunks.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, 0, &[]).await?;
            chunks.push(ci);
        }

        let data_chunk_count = chunks.len() as u32;
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

        let etag = format!("\"{}\"", hex::encode(md5_hasher.finalize()));
        let checksum_value = checksum_hasher.map(|h| h.finalize_base64());
        let storage_format = if has_parity {
            Some("chunked-v2".to_string())
        } else {
            Some("chunked-v1".to_string())
        };

        tmp_ec_guard.disarm();
        Ok(WrittenPayload {
            size: total_size,
            etag,
            checksum_algorithm: checksum_algo,
            checksum_value,
            storage_format,
            tmp_path: tmp_ec_dir,
            final_path: ec_dir,
            payload_is_dir: true,
            published: false,
        })
    }

    pub async fn discard_payload(path: &Path, is_dir: bool) -> Result<(), StorageError> {
        remove_path_if_exists(path, is_dir).await;
        Ok(())
    }

    pub async fn publish_temp_payload(
        tmp_payload: &Path,
        final_payload: &Path,
        payload_is_dir: bool,
    ) -> Result<(), StorageError> {
        if let Some(parent) = final_payload.parent() {
            fs::create_dir_all(parent).await?;
        }

        let payload_backup = backup_existing(final_payload).await?;

        if let Err(e) = fs::rename(tmp_payload, final_payload).await {
            restore_backup(final_payload, &payload_backup, payload_is_dir).await;
            return Err(StorageError::Io(e));
        }

        cleanup_backup(&payload_backup, payload_is_dir).await;
        Ok(())
    }

    pub async fn open_object(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
    ) -> Result<ByteStream, StorageError> {
        let ec_dir = self.ec_dir(bucket, key);
        if is_chunked_path(&ec_dir).await {
            let manifest = self.read_manifest(bucket, key).await?;
            let reader = VerifiedChunkReader::new(ec_dir, manifest);
            return Ok(Box::pin(reader));
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
            return Ok(Box::pin(std::io::Cursor::new(data)));
        }
        let file = fs::File::open(&obj_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(Box::pin(BufReader::with_capacity(
            IO_BUFFER_SIZE,
            file,
        )))
    }

    pub async fn open_object_range(
        &self,
        bucket: &str,
        key: &str,
        _meta: &ObjectMeta,
        offset: u64,
        length: u64,
    ) -> Result<ByteStream, StorageError> {
        let ec_dir = self.ec_dir(bucket, key);
        if is_chunked_path(&ec_dir).await {
            let manifest = self.read_manifest(bucket, key).await?;
            let reader = VerifiedChunkReader::with_range(ec_dir, manifest, offset, length);
            return Ok(Box::pin(reader));
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
            return Ok(Box::pin(std::io::Cursor::new(data)));
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
        Ok(Box::pin(BufReader::with_capacity(
            IO_BUFFER_SIZE,
            limited,
        )))
    }

    pub async fn unlink_object(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let obj_path = self.object_path(bucket, key);
        let ec_dir = self.ec_dir(bucket, key);
        remove_file_if_exists(&obj_path).await?;
        remove_dir_all_if_exists(&ec_dir).await?;
        self.cleanup_empty_parents(bucket, key).await;
        Ok(())
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

    pub async fn assemble_multipart_flat_temp(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[PartMeta],
    ) -> Result<WrittenPayload, StorageError> {
        let obj_path = self.object_path(bucket, key);
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

        for part in parts {
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
            parts.len()
        );

        tmp_obj_guard.disarm();
        Ok(WrittenPayload {
            size: total_size,
            etag,
            checksum_algorithm: None,
            checksum_value: None,
            storage_format: None,
            tmp_path: tmp_obj_path,
            final_path: obj_path,
            payload_is_dir: false,
            published: false,
        })
    }

    pub async fn assemble_multipart_chunked_temp(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[PartMeta],
    ) -> Result<WrittenPayload, StorageError> {
        let ec_dir = self.ec_dir(bucket, key);
        let tmp_ec_dir = temp_sibling_path(&ec_dir);
        let mut tmp_ec_guard = TempPathGuard::dir(tmp_ec_dir.clone());
        if let Some(parent) = ec_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::create_dir_all(&tmp_ec_dir).await?;

        let mut total_size = 0u64;
        let mut etag_hasher = Md5::new();
        let mut chunks: Vec<ChunkInfo> = Vec::new();
        let mut chunk_index: u32 = 0;
        let mut chunk_buf = Vec::with_capacity(self.chunk_size as usize);
        let mut buf = vec![0u8; IO_BUFFER_SIZE];

        for part in parts {
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

        if !chunk_buf.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_buf).await?;
            chunks.push(ci);
        }

        if chunks.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, 0, &[]).await?;
            chunks.push(ci);
        }

        let data_chunk_count = chunks.len() as u32;
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
            parts.len()
        );
        let storage_format = if has_parity {
            Some("chunked-v2".to_string())
        } else {
            Some("chunked-v1".to_string())
        };

        tmp_ec_guard.disarm();
        Ok(WrittenPayload {
            size: total_size,
            etag,
            checksum_algorithm: None,
            checksum_value: None,
            storage_format,
            tmp_path: tmp_ec_dir,
            final_path: ec_dir,
            payload_is_dir: true,
            published: false,
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

    pub async fn remove_upload_dir(&self, bucket: &str, upload_id: &str) -> Result<(), StorageError> {
        remove_dir_all_if_exists(&self.upload_dir(bucket, upload_id)).await
    }

    pub fn generate_version_id() -> String {
        let micros = chrono::Utc::now().timestamp_micros() as u64;
        let rand_suffix: u32 = rand::rng().random();
        format!("{:016}-{:08x}", micros, rand_suffix)
    }

    pub async fn archive_version_flat(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        data_path: &Path,
    ) -> Result<(), StorageError> {
        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;
        fs::copy(data_path, self.version_data_path(bucket, key, version_id))
            .await?;
        Ok(())
    }

    pub async fn archive_version_chunked(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;
        let src_ec = self.ec_dir(bucket, key);
        let dst_ec = self.version_ec_dir(bucket, key, version_id);
        fs::create_dir_all(&dst_ec).await?;
        let mut entries = fs::read_dir(&src_ec).await?;
        while let Some(entry) = entries.next_entry().await? {
            fs::copy(entry.path(), dst_ec.join(entry.file_name())).await?;
        }
        Ok(())
    }

    pub async fn unlink_version_blobs(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        let _ = fs::remove_file(self.version_data_path(bucket, key, version_id)).await;
        let _ = fs::remove_dir_all(self.version_ec_dir(bucket, key, version_id)).await;
        let ver_dir = self.versions_dir(bucket, key);
        let _ = fs::remove_dir(&ver_dir).await;
        Ok(())
    }

    pub async fn open_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ByteStream, StorageError> {
        let ver_ec_dir = self.version_ec_dir(bucket, key, version_id);
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
            return Ok(Box::pin(reader));
        }

        let ver_data_path = self.version_data_path(bucket, key, version_id);
        let file = fs::File::open(&ver_data_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(Box::pin(BufReader::with_capacity(
            IO_BUFFER_SIZE,
            file,
        )))
    }

    pub async fn restore_current_from_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        storage_format: Option<&str>,
    ) -> Result<(), StorageError> {
        let is_chunked = storage_format
            .map(|f| f.starts_with("chunked"))
            .unwrap_or(false);
        if is_chunked {
            let ver_ec = self.version_ec_dir(bucket, key, version_id);
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
            let ver_data = self.version_data_path(bucket, key, version_id);
            let obj_path = self.object_path(bucket, key);
            if let Some(parent) = obj_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&ver_data, &obj_path).await?;
        }
        Ok(())
    }

    pub async fn housekeeping_temp_sweep(&self) -> u64 {
        let mut temp_removed = 0u64;
        let mut bucket_entries = match fs::read_dir(&self.buckets_dir).await {
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

    async fn compute_and_write_parity_in_dir(
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
                k, m, k + m
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

async fn is_chunked_path(ec_dir: &Path) -> bool {
    matches!(fs::metadata(ec_dir).await, Ok(m) if m.is_dir())
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
    let mut file = fs::File::create(path).await?;
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
