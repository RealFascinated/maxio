use maxio::storage::ChecksumAlgorithm;
use maxio::storage::checksum::ChecksumHasher;

#[test]
fn crc32_matches_aws_test_vector() {
    let mut hasher = ChecksumHasher::new(ChecksumAlgorithm::CRC32);
    hasher.update(b"123456789");
    assert_eq!(hasher.finalize_base64(), "y/Q5Jg==");
}

#[test]
fn crc32c_matches_aws_test_vector() {
    let mut hasher = ChecksumHasher::new(ChecksumAlgorithm::CRC32C);
    hasher.update(b"123456789");
    assert_eq!(hasher.finalize_base64(), "4waSgw==");
}

#[test]
fn crc64nvme_matches_aws_test_vector() {
    let mut hasher = ChecksumHasher::new(ChecksumAlgorithm::CRC64NVME);
    hasher.update(b"123456789");
    let b64 = hasher.finalize_base64();
    // CRC-64/NVME of "123456789" — same value AWS SDK uses (big-endian u64, base64).
    assert_eq!(b64, "rosUhgp5mIg=");
}

#[test]
fn checksum_algorithms_have_names_and_headers() {
    for algo in ChecksumAlgorithm::all() {
        assert!(!algo.db_name().is_empty());
        assert!(!algo.header_name().is_empty());
    }
}
