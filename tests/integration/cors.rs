use crate::common::*;

// ---- CORS API tests ----

const CORS_XML_WILDCARD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration>
  <CORSRule>
    <AllowedOrigin>*</AllowedOrigin>
    <AllowedMethod>GET</AllowedMethod>
    <AllowedMethod>PUT</AllowedMethod>
    <AllowedHeader>*</AllowedHeader>
    <MaxAgeSeconds>3600</MaxAgeSeconds>
  </CORSRule>
</CORSConfiguration>"#;

const CORS_XML_EXACT_ORIGIN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration>
  <CORSRule>
    <AllowedOrigin>http://example.com</AllowedOrigin>
    <AllowedMethod>GET</AllowedMethod>
  </CORSRule>
</CORSConfiguration>"#;

#[tokio::test]
async fn test_put_get_delete_bucket_cors() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-bucket", base), vec![]).await;

    // GetBucketCors on bucket with no CORS → 404 NoSuchCORSConfiguration
    let resp = s3_request("GET", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("NoSuchCORSConfiguration"));

    // PutBucketCors
    let resp = s3_request(
        "PUT",
        &format!("{}/cors-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    // GetBucketCors → should return config
    let resp = s3_request("GET", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("CORSConfiguration"));
    assert!(body.contains("AllowedMethod"));
    assert!(body.contains("GET"));

    // DeleteBucketCors
    let resp = s3_request("DELETE", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 204);

    // GetBucketCors after delete → 404 again
    let resp = s3_request("GET", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_put_cors_invalid_method() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-invalid", base), vec![]).await;

    let bad_cors = r#"<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration>
  <CORSRule>
    <AllowedOrigin>*</AllowedOrigin>
    <AllowedMethod>PATCH</AllowedMethod>
  </CORSRule>
</CORSConfiguration>"#;

    let resp = s3_request(
        "PUT",
        &format!("{}/cors-invalid?cors", base),
        bad_cors.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

#[tokio::test]
async fn test_cors_preflight_allowed() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/preflight-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/preflight-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;

    // OPTIONS preflight — should return 200 with CORS headers
    let resp = client()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/preflight-bucket/file.txt", base),
        )
        .header("Origin", "http://example.com")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let headers = resp.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
    assert!(headers.contains_key("access-control-allow-methods"));
}

#[tokio::test]
async fn test_cors_preflight_no_config_returns_403() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/no-cors-bucket", base), vec![]).await;

    let resp = client()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/no-cors-bucket/file.txt", base),
        )
        .header("Origin", "http://example.com")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_cors_preflight_unmatched_origin_returns_403() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/exact-origin-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/exact-origin-bucket?cors", base),
        CORS_XML_EXACT_ORIGIN.as_bytes().to_vec(),
    )
    .await;

    let resp = client()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/exact-origin-bucket/file.txt", base),
        )
        .header("Origin", "http://other.com")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_cors_normal_request_gets_headers() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-normal-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/cors-normal-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/cors-normal-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/cors-normal-bucket/obj.txt", base),
        vec![],
        vec![("origin", "http://example.com")],
    )
    .await;

    assert_eq!(resp.status(), 200);
    let headers = resp.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
    assert_eq!(
        headers
            .get("access-control-allow-origin")
            .unwrap()
            .to_str()
            .unwrap(),
        "http://example.com"
    );
}

#[tokio::test]
async fn test_cors_no_origin_no_cors_headers() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-noorigin-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/cors-noorigin-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/cors-noorigin-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;

    let resp = s3_request(
        "GET",
        &format!("{}/cors-noorigin-bucket/obj.txt", base),
        vec![],
    )
    .await;

    assert_eq!(resp.status(), 200);
    assert!(!resp.headers().contains_key("access-control-allow-origin"));
}
