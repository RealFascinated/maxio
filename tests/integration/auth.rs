use crate::common::*;

#[tokio::test]
async fn test_auth_rejects_bad_key() {
    let base_url = start_server().await;

    // Request with no auth header
    let resp = client().get(&*base_url).send().await.unwrap();
    assert_eq!(resp.status(), 403);

    // Request with garbage auth
    let resp = client()
        .get(&*base_url)
        .header("authorization", "garbage")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_auth_accepts_valid_signature() {
    let base_url = start_server().await;
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_verify_with_signing_key_roundtrip() {
    use axum::http::{HeaderMap, HeaderName, HeaderValue};
    use maxio::auth::signature_v4::{
        derive_signing_key, parse_authorization_header, verify_with_signing_key,
    };

    let base_url = start_server().await;
    let url = format!("{}/", base_url);
    let mut headers = Vec::new();
    sign_request("GET", &url, &mut headers, b"");

    let parsed_url = reqwest::Url::parse(&url).unwrap();
    let mut header_map = HeaderMap::new();
    let mut auth_value = String::new();
    for (k, v) in headers {
        if k == "authorization" {
            auth_value = v;
            continue;
        }
        header_map.insert(
            HeaderName::from_bytes(k.as_bytes()).unwrap(),
            HeaderValue::from_str(&v).unwrap(),
        );
    }
    let parsed = parse_authorization_header(&auth_value).unwrap();
    let key = derive_signing_key(SECRET_KEY, &parsed.date, REGION);
    assert!(verify_with_signing_key(
        "GET",
        parsed_url.path(),
        parsed_url.query().unwrap_or(""),
        &header_map,
        &parsed,
        &key,
    ));
}

#[tokio::test]
async fn test_auth_compact_header_no_spaces() {
    // mc sends Authorization header with commas but no spaces:
    // Credential=...,SignedHeaders=...,Signature=...
    let base_url = start_server().await;

    let resp = s3_request_compact("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // Also test PUT bucket with compact header
    let resp = s3_request_compact("PUT", &format!("{}/compact-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}
