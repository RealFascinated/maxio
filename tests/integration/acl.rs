use crate::common::*;

#[tokio::test]
async fn test_bucket_acl_canned_public_read_roundtrip() {
    let base_url = start_server().await;
    let bucket = "acl-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let put_acl = s3_request_with_headers(
        "PUT",
        &format!("{}/{bucket}?acl", base_url),
        vec![],
        vec![("x-amz-acl", "public-read")],
    )
    .await;
    assert_eq!(put_acl.status(), 200);

    let get_acl = s3_request("GET", &format!("{}/{bucket}?acl", base_url), vec![]).await;
    assert_eq!(get_acl.status(), 200);
    let body = get_acl.text().await.unwrap();
    assert!(body.contains("AllUsers"));
    assert!(body.contains("READ"));
}

#[tokio::test]
async fn test_object_acl_get_after_put() {
    let base_url = start_server().await;
    let bucket = "obj-acl-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/file.txt", base_url),
            b"data".to_vec(),
        )
        .await
        .status(),
        200
    );

    let put_acl = s3_request_with_headers(
        "PUT",
        &format!("{}/{bucket}/file.txt?acl", base_url),
        vec![],
        vec![("x-amz-acl", "private")],
    )
    .await;
    assert_eq!(put_acl.status(), 200);

    let get_acl = s3_request(
        "GET",
        &format!("{}/{bucket}/file.txt?acl", base_url),
        vec![],
    )
    .await;
    assert_eq!(get_acl.status(), 200);
    let body = get_acl.text().await.unwrap();
    assert!(body.contains("Owner"));
}
