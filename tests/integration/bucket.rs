use tempfile::TempDir;

use crate::common::*;

#[tokio::test]
async fn test_create_bucket() {
    let base_url = start_server().await;

    // Create bucket
    let resp = s3_request("PUT", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // Head bucket should succeed
    let resp = s3_request("HEAD", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_create_bucket_rejects_canonical_invalid_names() {
    let base_url = start_server().await;

    for bucket in ["a.-b", "a-.b", "192.168.0.1"] {
        let resp = s3_request("PUT", &format!("{}/{}", base_url, bucket), vec![]).await;
        assert_eq!(resp.status(), 400, "{bucket} should be rejected");
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("<Code>InvalidBucketName</Code>"),
            "{bucket} should return InvalidBucketName, got {body}"
        );
    }
}

#[tokio::test]
async fn test_create_bucket_duplicate() {
    let base_url = start_server().await;

    let resp = s3_request("PUT", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // Creating same bucket again should fail
    let resp = s3_request("PUT", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn test_head_bucket_not_found() {
    let base_url = start_server().await;

    let resp = s3_request("HEAD", &format!("{}/nonexistent", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_list_buckets() {
    let base_url = start_server().await;

    // Create two buckets
    s3_request("PUT", &format!("{}/alpha", base_url), vec![]).await;
    s3_request("PUT", &format!("{}/beta", base_url), vec![]).await;

    // List
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Name>alpha</Name>"));
    assert!(body.contains("<Name>beta</Name>"));
}

#[tokio::test]
async fn test_delete_bucket() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/to-delete", base_url), vec![]).await;

    let resp = s3_request("DELETE", &format!("{}/to-delete", base_url), vec![]).await;
    assert_eq!(resp.status(), 204);

    // Should be gone
    let resp = s3_request("HEAD", &format!("{}/to-delete", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
}

// Regression: delete_bucket must succeed after full object lifecycle
// (put + delete) even when metadata sidecars or empty dirs remain.
#[tokio::test]
async fn test_delete_bucket_after_object_lifecycle() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/bucket-one", base_url), vec![]).await;
    let r = s3_request(
        "PUT",
        &format!("{}/bucket-one/f.txt", base_url),
        b"x".to_vec(),
    )
    .await;
    assert_eq!(r.status(), 200);
    let r = s3_request("DELETE", &format!("{}/bucket-one/f.txt", base_url), vec![]).await;
    assert_eq!(r.status(), 204);

    let r = s3_request("DELETE", &format!("{}/bucket-one", base_url), vec![]).await;
    assert_eq!(
        r.status(),
        204,
        "bucket delete should succeed after object removed"
    );

    let r = s3_request("HEAD", &format!("{}/bucket-one", base_url), vec![]).await;
    assert_eq!(r.status(), 404);
}

// Regression: nested keys leave deep directory trees; delete_bucket must
// sweep empty parents.
#[tokio::test]
async fn test_delete_bucket_with_nested_path() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/bucket-two", base_url), vec![]).await;
    let r = s3_request(
        "PUT",
        &format!("{}/bucket-two/a/b/c/d.txt", base_url),
        b"y".to_vec(),
    )
    .await;
    assert_eq!(r.status(), 200);
    let r = s3_request(
        "DELETE",
        &format!("{}/bucket-two/a/b/c/d.txt", base_url),
        vec![],
    )
    .await;
    assert_eq!(r.status(), 204);

    let r = s3_request("DELETE", &format!("{}/bucket-two", base_url), vec![]).await;
    assert_eq!(
        r.status(),
        204,
        "bucket delete should sweep empty nested dirs"
    );
}

// Ensure we did not weaken the real emptiness check.
#[tokio::test]
async fn test_delete_bucket_rejects_real_object() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/bucket-three", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/bucket-three/stay.txt", base_url),
        b"z".to_vec(),
    )
    .await;

    let r = s3_request("DELETE", &format!("{}/bucket-three", base_url), vec![]).await;
    assert_eq!(r.status(), 409);

    // Bucket still exists.
    let r = s3_request("HEAD", &format!("{}/bucket-three", base_url), vec![]).await;
    assert_eq!(r.status(), 200);
}

// Regression: stale nested `.versions/` dir (from past versioning state)
// must not block bucket deletion. Exercised directly against the storage
// layer so the test does not depend on the S3 versioning API.
#[tokio::test]
async fn test_delete_bucket_sweeps_nested_versions() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap().to_string();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(&data_dir, &database_url).await;

    storage
        .create_bucket(&maxio::storage::BucketMeta {
            name: "leftover".to_string(),
            created_at: "2026-04-16T00:00:00.000Z".to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: maxio::iam::ROOT_CANONICAL_ID.to_string(),
            owner_display_name: maxio::iam::ROOT_DISPLAY_NAME.to_string(),
            acl: Some(maxio::iam::Acl::private(
                maxio::iam::ROOT_CANONICAL_ID,
                maxio::iam::ROOT_DISPLAY_NAME,
            )),
            policy: None,
            public_read: false,
            public_list: false,
        })
        .await
        .unwrap();

    // Orphan on-disk artifacts must not block metadata-only bucket deletion.
    let bucket_root = tmp.path().join("buckets").join("leftover");
    let stale_versions = bucket_root.join("photos").join(".versions");
    tokio::fs::create_dir_all(&stale_versions).await.unwrap();
    tokio::fs::write(bucket_root.join("orphan.txt"), b"orphan bytes")
        .await
        .unwrap();

    let deleted = storage.delete_bucket("leftover").await.unwrap();
    assert!(
        deleted,
        "delete_bucket should succeed when metadata has no objects"
    );
    assert!(!storage.head_bucket("leftover").await.unwrap());
    let _postgres = postgres;
}
