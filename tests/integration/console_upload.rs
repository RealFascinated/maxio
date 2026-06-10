use crate::common::*;

#[tokio::test]
async fn test_console_upload_and_download() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "console-upload-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let upload = client()
        .put(format!(
            "{}/api/buckets/{bucket}/upload/hello.txt",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .header("content-type", "text/plain")
        .body("hello console".to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 200);

    let download = client()
        .get(format!(
            "{}/api/buckets/{bucket}/download/hello.txt",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(download.status(), 200);
    assert_eq!(&download.bytes().await.unwrap()[..], b"hello console");
}

#[tokio::test]
async fn test_console_bucket_versioning_settings() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "console-versioning-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let get = client()
        .get(format!("{}/api/buckets/{bucket}/versioning", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let body: serde_json::Value = get.json().await.unwrap();
    assert_eq!(body["enabled"], false);

    let set = client()
        .put(format!("{}/api/buckets/{bucket}/versioning", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(set.status(), 200);

    let s3 = s3_request("GET", &format!("{}/{bucket}?versioning", base_url), vec![]).await;
    assert_eq!(s3.status(), 200);
    let xml = s3.text().await.unwrap();
    assert!(xml.contains("<Status>Enabled</Status>"), "{}", xml);
}

#[tokio::test]
async fn test_console_bucket_public_settings() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "console-public-bucket";

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
            b"public via console".to_vec(),
        )
        .await
        .status(),
        200
    );

    let set = client()
        .put(format!("{}/api/buckets/{bucket}/public", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({"read": true, "list": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(set.status(), 200);

    let anon = client()
        .get(format!("{}/{bucket}/open.txt", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 200);
    assert_eq!(&anon.bytes().await.unwrap()[..], b"public via console");
}
