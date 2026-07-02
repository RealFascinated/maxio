use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    CRC32,
    CRC32C,
    CRC64NVME,
    SHA1,
    SHA256,
}

impl ChecksumAlgorithm {
    pub fn header_name(self) -> &'static str {
        self.codec().request_header()
    }

    pub fn from_header_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "CRC32" => Some(Self::CRC32),
            "CRC32C" => Some(Self::CRC32C),
            "CRC64NVME" => Some(Self::CRC64NVME),
            "SHA1" => Some(Self::SHA1),
            "SHA256" => Some(Self::SHA256),
            _ => None,
        }
    }

    pub fn all() -> &'static [ChecksumAlgorithm] {
        &[
            ChecksumAlgorithm::CRC32,
            ChecksumAlgorithm::CRC32C,
            ChecksumAlgorithm::CRC64NVME,
            ChecksumAlgorithm::SHA1,
            ChecksumAlgorithm::SHA256,
        ]
    }

    pub fn from_request_header(header: &str) -> Option<Self> {
        let lower = header.to_ascii_lowercase();
        for algo in Self::all() {
            if algo.header_name() == lower {
                return Some(*algo);
            }
        }
        None
    }

    pub fn db_name(self) -> &'static str {
        self.codec().id()
    }

    pub fn composite_multipart(self, part_checksums: &[String]) -> Option<String> {
        self.codec().composite_multipart(part_checksums)
    }

    pub fn codec(self) -> &'static dyn super::codec::ChecksumCodec {
        match self {
            Self::CRC32 => &super::codec::CRC32,
            Self::CRC32C => &super::codec::CRC32C,
            Self::CRC64NVME => &super::codec::CRC64_NVME,
            Self::SHA1 => &super::codec::SHA1,
            Self::SHA256 => &super::codec::SHA256,
        }
    }
}
