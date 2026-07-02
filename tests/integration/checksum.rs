use maxio::storage::ChecksumAlgorithm;
use maxio::storage::checksum::ChecksumHasher;

use crate::common::*;

fn checksum_b64(data: &[u8], algo: ChecksumAlgorithm) -> String {
    let mut hasher = ChecksumHasher::new(algo);
    hasher.update(data);
    hasher.finalize_base64()
}

#[tokio::test]
async fn test_put_object_crc32_checksum_roundtrip() {
    let base_url = start_server().await;
    let bucket = "checksum-bucket";
    let data = b"checksum test data";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let crc32 = checksum_b64(data, ChecksumAlgorithm::CRC32);
    let put = s3_request_with_headers(
        "PUT",
        &format!("{}/{bucket}/checksum.txt", base_url),
        data.to_vec(),
        vec![
            ("x-amz-checksum-algorithm", "CRC32"),
            ("x-amz-checksum-crc32", &crc32),
        ],
    )
    .await;
    assert_eq!(put.status(), 200);

    let head = s3_request(
        "HEAD",
        &format!("{}/{bucket}/checksum.txt", base_url),
        vec![],
    )
    .await;
    assert_eq!(head.status(), 200);
    assert_eq!(
        head.headers()
            .get("x-amz-checksum-crc32")
            .unwrap()
            .to_str()
            .unwrap(),
        crc32
    );
}

#[tokio::test]
async fn test_put_object_wrong_crc32_checksum_rejected() {
    let base_url = start_server().await;
    let bucket = "checksum-bad-bucket";
    let data = b"checksum test data";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let put = s3_request_with_headers(
        "PUT",
        &format!("{}/{bucket}/bad.txt", base_url),
        data.to_vec(),
        vec![
            ("x-amz-checksum-algorithm", "CRC32"),
            ("x-amz-checksum-crc32", "AAAAAAAA"),
        ],
    )
    .await;
    assert_eq!(put.status(), 400);
    let body = put.text().await.unwrap();
    assert!(body.contains("BadDigest"), "{}", body);
}

#[tokio::test]
async fn test_put_object_sha256_checksum_accepted() {
    let base_url = start_server().await;
    let bucket = "checksum-sha-bucket";
    let data = b"checksum test data";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let sha256 = checksum_b64(data, ChecksumAlgorithm::SHA256);
    let put = s3_request_with_headers(
        "PUT",
        &format!("{}/{bucket}/sha256.txt", base_url),
        data.to_vec(),
        vec![
            ("x-amz-checksum-algorithm", "SHA256"),
            ("x-amz-checksum-sha256", &sha256),
        ],
    )
    .await;
    assert_eq!(put.status(), 200);
}

#[tokio::test]
async fn test_put_object_crc64nvme_checksum_roundtrip() {
    let base_url = start_server().await;
    let bucket = "checksum-crc64-bucket";
    let data = b"checksum test data";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let crc64 = checksum_b64(data, ChecksumAlgorithm::CRC64NVME);
    let put = s3_request_with_headers(
        "PUT",
        &format!("{}/{bucket}/crc64.txt", base_url),
        data.to_vec(),
        vec![
            ("x-amz-checksum-algorithm", "CRC64NVME"),
            ("x-amz-checksum-crc64nvme", &crc64),
        ],
    )
    .await;
    assert_eq!(put.status(), 200);

    let head = s3_request("HEAD", &format!("{}/{bucket}/crc64.txt", base_url), vec![]).await;
    assert_eq!(head.status(), 200);
    assert_eq!(
        head.headers()
            .get("x-amz-checksum-crc64nvme")
            .unwrap()
            .to_str()
            .unwrap(),
        crc64
    );
}
