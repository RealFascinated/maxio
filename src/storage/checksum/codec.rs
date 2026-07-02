use base64::Engine;

use super::algorithm::ChecksumAlgorithm;

/// Incremental checksum computation for a single S3 algorithm.
pub trait IncrementalChecksum: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize_base64(self: Box<Self>) -> String;
}

/// Contract for an S3 request/response checksum algorithm.
pub trait ChecksumCodec: Send + Sync {
    fn id(&self) -> &'static str;
    fn request_header(&self) -> &'static str;
    fn new_hasher(&self) -> Box<dyn IncrementalChecksum + Send>;
    fn supports_multipart_composite(&self) -> bool;
    fn composite_multipart(&self, part_checksums: &[String]) -> Option<String> {
        if !self.supports_multipart_composite() {
            return None;
        }
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut raw = Vec::new();
        for val in part_checksums {
            if let Ok(bytes) = b64.decode(val) {
                raw.extend_from_slice(&bytes);
            }
        }
        if raw.is_empty() {
            return None;
        }
        let mut hasher = self.new_hasher();
        hasher.update(&raw);
        Some(format!(
            "{}-{}",
            hasher.finalize_base64(),
            part_checksums.len()
        ))
    }
}

pub struct Crc32;
pub struct Crc32c;
pub struct Crc64Nvme;
pub struct Sha1;
pub struct Sha256;

struct CrcFastHasher {
    digest: crc_fast::Digest,
    width: u8,
}

struct Sha1Hasher(aws_lc_rs::digest::Context);
struct Sha256Hasher(aws_lc_rs::digest::Context);

impl CrcFastHasher {
    fn new(algorithm: crc_fast::CrcAlgorithm, width: u8) -> Self {
        Self {
            digest: crc_fast::Digest::new(algorithm),
            width,
        }
    }

    fn finalize_bytes(&self) -> Vec<u8> {
        let raw = self.digest.finalize();
        match self.width {
            32 => (raw as u32).to_be_bytes().to_vec(),
            64 => raw.to_be_bytes().to_vec(),
            _ => unreachable!(),
        }
    }
}

impl IncrementalChecksum for CrcFastHasher {
    fn update(&mut self, data: &[u8]) {
        self.digest.update(data);
    }
    fn finalize_base64(self: Box<Self>) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.finalize_bytes())
    }
}

impl IncrementalChecksum for Sha1Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize_base64(self: Box<Self>) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.0.finish().as_ref())
    }
}

impl IncrementalChecksum for Sha256Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize_base64(self: Box<Self>) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.0.finish().as_ref())
    }
}

impl ChecksumCodec for Crc32 {
    fn id(&self) -> &'static str {
        "CRC32"
    }
    fn request_header(&self) -> &'static str {
        "x-amz-checksum-crc32"
    }
    fn new_hasher(&self) -> Box<dyn IncrementalChecksum + Send> {
        Box::new(CrcFastHasher::new(crc_fast::CrcAlgorithm::Crc32IsoHdlc, 32))
    }
    fn supports_multipart_composite(&self) -> bool {
        true
    }
}

impl ChecksumCodec for Crc32c {
    fn id(&self) -> &'static str {
        "CRC32C"
    }
    fn request_header(&self) -> &'static str {
        "x-amz-checksum-crc32c"
    }
    fn new_hasher(&self) -> Box<dyn IncrementalChecksum + Send> {
        Box::new(CrcFastHasher::new(crc_fast::CrcAlgorithm::Crc32Iscsi, 32))
    }
    fn supports_multipart_composite(&self) -> bool {
        true
    }
}

impl ChecksumCodec for Crc64Nvme {
    fn id(&self) -> &'static str {
        "CRC64NVME"
    }
    fn request_header(&self) -> &'static str {
        "x-amz-checksum-crc64nvme"
    }
    fn new_hasher(&self) -> Box<dyn IncrementalChecksum + Send> {
        Box::new(CrcFastHasher::new(crc_fast::CrcAlgorithm::Crc64Nvme, 64))
    }
    fn supports_multipart_composite(&self) -> bool {
        true
    }
}

impl ChecksumCodec for Sha1 {
    fn id(&self) -> &'static str {
        "SHA1"
    }
    fn request_header(&self) -> &'static str {
        "x-amz-checksum-sha1"
    }
    fn new_hasher(&self) -> Box<dyn IncrementalChecksum + Send> {
        Box::new(Sha1Hasher(aws_lc_rs::digest::Context::new(
            &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
        )))
    }
    fn supports_multipart_composite(&self) -> bool {
        false
    }
}

impl ChecksumCodec for Sha256 {
    fn id(&self) -> &'static str {
        "SHA256"
    }
    fn request_header(&self) -> &'static str {
        "x-amz-checksum-sha256"
    }
    fn new_hasher(&self) -> Box<dyn IncrementalChecksum + Send> {
        Box::new(Sha256Hasher(aws_lc_rs::digest::Context::new(
            &aws_lc_rs::digest::SHA256,
        )))
    }
    fn supports_multipart_composite(&self) -> bool {
        false
    }
}

pub static CRC32: Crc32 = Crc32;
pub static CRC32C: Crc32c = Crc32c;
pub static CRC64_NVME: Crc64Nvme = Crc64Nvme;
pub static SHA1: Sha1 = Sha1;
pub static SHA256: Sha256 = Sha256;

/// Stateful hasher selected by algorithm — used on the storage write path.
pub struct ChecksumHasher {
    inner: Box<dyn IncrementalChecksum + Send>,
}

impl ChecksumHasher {
    pub fn new(algo: ChecksumAlgorithm) -> Self {
        Self {
            inner: algo.codec().new_hasher(),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub fn finalize_base64(self) -> String {
        self.inner.finalize_base64()
    }
}
