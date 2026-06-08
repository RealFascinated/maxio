use aws_lc_rs::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY, SHA256};
use aws_lc_sys::{MD5_CTX, MD5_Final, MD5_Init, MD5_Update};
use base64::Engine;

use super::ChecksumAlgorithm;

/// Incremental MD5 for S3 etags — backed by AWS-LC assembly implementations.
pub struct EtagMd5 {
    ctx: MD5_CTX,
}

impl EtagMd5 {
    pub fn new() -> Self {
        let mut ctx = std::mem::MaybeUninit::uninit();
        // SAFETY: MD5_Init always succeeds for a valid context pointer.
        unsafe {
            MD5_Init(ctx.as_mut_ptr());
            Self {
                ctx: ctx.assume_init(),
            }
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        // SAFETY: ctx is initialized; data pointer is valid for data.len() bytes.
        unsafe {
            MD5_Update(&mut self.ctx, data.as_ptr().cast(), data.len());
        }
    }

    pub fn finalize(mut self) -> [u8; 16] {
        let mut out = [0u8; 16];
        // SAFETY: ctx is initialized; out is 16 bytes as required by MD5_Final.
        unsafe {
            MD5_Final(out.as_mut_ptr(), &mut self.ctx);
        }
        out
    }
}

enum ChecksumHasherInner {
    Crc32(crc32fast::Hasher),
    Crc32c(u32),
    Sha1(Context),
    Sha256(Context),
}

pub struct ChecksumHasher(ChecksumHasherInner);

impl ChecksumHasher {
    pub fn new(algo: ChecksumAlgorithm) -> Self {
        Self(match algo {
            ChecksumAlgorithm::CRC32 => ChecksumHasherInner::Crc32(crc32fast::Hasher::new()),
            ChecksumAlgorithm::CRC32C => ChecksumHasherInner::Crc32c(0),
            ChecksumAlgorithm::SHA1 => {
                ChecksumHasherInner::Sha1(Context::new(&SHA1_FOR_LEGACY_USE_ONLY))
            }
            ChecksumAlgorithm::SHA256 => ChecksumHasherInner::Sha256(Context::new(&SHA256)),
        })
    }

    pub fn update(&mut self, data: &[u8]) {
        match &mut self.0 {
            ChecksumHasherInner::Crc32(h) => h.update(data),
            ChecksumHasherInner::Crc32c(v) => *v = crc32c::crc32c_append(*v, data),
            ChecksumHasherInner::Sha1(ctx) => ctx.update(data),
            ChecksumHasherInner::Sha256(ctx) => ctx.update(data),
        }
    }

    pub fn finalize_base64(self) -> String {
        let b64 = base64::engine::general_purpose::STANDARD;
        match self.0 {
            ChecksumHasherInner::Crc32(h) => b64.encode(h.finalize().to_be_bytes()),
            ChecksumHasherInner::Crc32c(v) => b64.encode(v.to_be_bytes()),
            ChecksumHasherInner::Sha1(ctx) => b64.encode(ctx.finish().as_ref()),
            ChecksumHasherInner::Sha256(ctx) => b64.encode(ctx.finish().as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_md5_known_vector() {
        let mut hasher = EtagMd5::new();
        hasher.update(b"hello maxio");
        assert_eq!(
            hex::encode(hasher.finalize()),
            "c3dd79c5d3cff40236ff7108f804f3ef"
        );
    }
}
