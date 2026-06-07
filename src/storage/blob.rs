use super::{ByteStream, ChecksumAlgorithm, ObjectMeta, PartMeta, StorageError};
use base64::Engine;
use md5::{Digest, Md5};
use rand::RngExt;
use std::path::{Component, Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter};

pub(crate) const IO_BUFFER_SIZE: usize = 256 * 1024;
pub(crate) const FLAT_WRITE_BUFFER_SIZE: usize = 8 * 1024;
pub(crate) const SMALL_OBJECT_THRESHOLD: u64 = 256 * 1024;

pub struct BlobStorage {
    pub(crate) buckets_dir: PathBuf,
}

pub struct WrittenPayload {
    pub size: u64,
    pub etag: String,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub checksum_value: Option<String>,
    pub tmp_path: PathBuf,
    pub final_path: PathBuf,
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
    pub async fn new(data_dir: &str) -> Result<Self, anyhow::Error> {
        let buckets_dir = Path::new(data_dir).join("buckets");
        fs::create_dir_all(&buckets_dir).await?;
        Ok(Self { buckets_dir })
    }

    pub fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir.join(bucket).join(dir).join(".folder")
        } else {
            self.buckets_dir.join(bucket).join(key)
        }
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
            Some(TempPathGuard::new(write_path.clone()))
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
            tmp_path: write_path,
            final_path: obj_path,
            published: direct_write,
        })
    }

    pub async fn discard_payload(path: &Path) -> Result<(), StorageError> {
        let _ = fs::remove_file(path).await;
        Ok(())
    }

    pub async fn publish_temp_payload(
        tmp_payload: &Path,
        final_payload: &Path,
    ) -> Result<(), StorageError> {
        if let Some(parent) = final_payload.parent() {
            fs::create_dir_all(parent).await?;
        }

        let payload_backup = backup_existing(final_payload).await?;

        if let Err(e) = fs::rename(tmp_payload, final_payload).await {
            restore_backup(final_payload, &payload_backup).await;
            return Err(StorageError::Io(e));
        }

        cleanup_backup(&payload_backup).await;
        Ok(())
    }

    pub async fn open_object(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
    ) -> Result<ByteStream, StorageError> {
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
        Ok(Box::pin(BufReader::with_capacity(IO_BUFFER_SIZE, limited)))
    }

    pub async fn unlink_object(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let obj_path = self.object_path(bucket, key);
        remove_file_if_exists(&obj_path).await?;
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

    pub async fn assemble_multipart_temp(
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
        let mut tmp_obj_guard = TempPathGuard::new(tmp_obj_path.clone());
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
            tmp_path: tmp_obj_path,
            final_path: obj_path,
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

async fn backup_existing(path: &Path) -> Result<Option<PathBuf>, StorageError> {
    if !fs::try_exists(path).await? {
        return Ok(None);
    }
    let backup = temp_sibling_path(path);
    fs::rename(path, &backup).await?;
    Ok(Some(backup))
}

async fn restore_backup(final_path: &Path, backup: &Option<PathBuf>) {
    if let Some(backup) = backup {
        let _ = fs::remove_file(final_path).await;
        let _ = fs::rename(backup, final_path).await;
    }
}

async fn cleanup_backup(backup: &Option<PathBuf>) {
    if let Some(backup) = backup {
        let _ = fs::remove_file(backup).await;
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
