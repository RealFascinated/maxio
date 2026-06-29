use crate::common::*;

#[tokio::test]
async fn test_bucket_policy_put_get_delete() {
    let base_url = start_server().await;
    let bucket = "policy-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::policy-bucket/*"}]}"#;
    let put = s3_request(
        "PUT",
        &format!("{}/{bucket}?policy", base_url),
        policy.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(put.status(), 204);

    let get = s3_request("GET", &format!("{}/{bucket}?policy", base_url), vec![]).await;
    assert_eq!(get.status(), 200);
    let body = get.text().await.unwrap();
    assert!(body.contains("s3:GetObject"));

    let status = s3_request(
        "GET",
        &format!("{}/{bucket}?policy-status", base_url),
        vec![],
    )
    .await;
    assert_eq!(status.status(), 200);
    let status_body = status.text().await.unwrap();
    assert!(status_body.contains("true"));

    assert_eq!(
        s3_request("DELETE", &format!("{}/{bucket}?policy", base_url), vec![])
            .await
            .status(),
        204
    );

    let get_after = s3_request("GET", &format!("{}/{bucket}?policy", base_url), vec![]).await;
    assert_eq!(get_after.status(), 404);
}

#[tokio::test]
async fn test_public_bucket_policy_allows_anonymous_get() {
    let base_url = start_server().await;
    let bucket = "public-read-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/open.txt", base_url),
            b"public data".to_vec(),
        )
        .await
        .status(),
        200
    );

    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::public-read-bucket/*"}]}"#;
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}?policy", base_url),
            policy.as_bytes().to_vec(),
        )
        .await
        .status(),
        204
    );

    let anon = client()
        .get(format!("{}/{bucket}/open.txt", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 200);
    assert_eq!(&anon.bytes().await.unwrap()[..], b"public data");
}
