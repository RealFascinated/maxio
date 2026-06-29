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

#[tokio::test]
async fn test_bucket_policy_missing_principal_rejected() {
    let base_url = start_server().await;
    let bucket = "policy-no-principal";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"arn:aws:s3:::policy-no-principal/*"}]}"#;
    let put = s3_request(
        "PUT",
        &format!("{}/{bucket}?policy", base_url),
        policy.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(put.status(), 400);
    let body = put.text().await.unwrap();
    assert!(body.contains("MalformedPolicy"));
}

#[tokio::test]
async fn test_bucket_policy_wrong_bucket_resource_rejected() {
    let base_url = start_server().await;
    let bucket = "policy-wrong-resource";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::other-bucket/*"}]}"#;
    let put = s3_request(
        "PUT",
        &format!("{}/{bucket}?policy", base_url),
        policy.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(put.status(), 400);
    let body = put.text().await.unwrap();
    assert!(body.contains("MalformedPolicy"));
}

#[tokio::test]
async fn test_bucket_policy_delete_without_policy_returns_404() {
    let base_url = start_server().await;
    let bucket = "policy-delete-missing";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let delete = s3_request("DELETE", &format!("{}/{bucket}?policy", base_url), vec![]).await;
    assert_eq!(delete.status(), 404);
    let body = delete.text().await.unwrap();
    assert!(body.contains("NoSuchBucketPolicy"));
}

#[tokio::test]
async fn test_console_bucket_policy_round_trip() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "console-policy-bucket";

    let create = client()
        .post(format!("{base_url}/api/buckets"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "name": bucket }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::console-policy-bucket/*"}]}"#;

    let put = client()
        .put(format!("{base_url}/api/buckets/{bucket}/policy"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "document": policy }))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 200);

    let get = client()
        .get(format!("{base_url}/api/buckets/{bucket}/policy"))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let body: serde_json::Value = get.json().await.unwrap();
    assert!(body["document"].as_str().unwrap().contains("s3:GetObject"));

    let delete = client()
        .delete(format!("{base_url}/api/buckets/{bucket}/policy"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);

    let get_after = client()
        .get(format!("{base_url}/api/buckets/{bucket}/policy"))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get_after.status(), 200);
    assert!(get_after.json::<serde_json::Value>().await.unwrap()["document"].is_null());
}
