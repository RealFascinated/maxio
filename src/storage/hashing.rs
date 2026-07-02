use aws_lc_sys::{MD5_CTX, MD5_Final, MD5_Init, MD5_Update};

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
