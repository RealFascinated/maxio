use crate::common::*;

async fn console_login(base_url: &str) -> String {
    let resp = client()
        .post(&format!("{}/api/auth/login", base_url))
        .json(&serde_json::json!({"accessKey": ACCESS_KEY, "secretKey": SECRET_KEY}))
        .send()
        .await
        .unwrap();
    if resp.status() != 200 {
        let status = resp.status();
        let body = resp.text().await.unwrap();
        panic!("login failed with status {}: {}", status, body);
    }
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .expect("login should set cookie")
        .to_str()
        .unwrap()
        .to_string();
    // Extract value from "maxio_session=VALUE; ..."
    let value = set_cookie
        .strip_prefix("maxio_session=")
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    value.to_string()
}

#[tokio::test]
async fn test_console_mutation_allows_dev_loopback_origin_via_vite_proxy() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let resp = client()
        .post(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={}", session))
        .header("origin", "http://127.0.0.1:5173")
        .json(&serde_json::json!({ "name": "dev-origin-bucket" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "body: {}", resp.text().await.unwrap());
}

#[tokio::test]
async fn test_console_mutation_rejects_cross_site_origin() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let resp = client()
        .post(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={}", session))
        .header("origin", "http://evil.example")
        .json(&serde_json::json!({ "name": "evil-origin-bucket" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_console_list_objects_search() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/search-bucket", base_url), vec![]).await;

    for key in [
        "alpha.txt",
        "folder/beta.txt",
        "folder/gamma-alpha.txt",
        "other.txt",
    ] {
        s3_request(
            "PUT",
            &format!("{}/search-bucket/{}", base_url, key),
            b"x".to_vec(),
        )
        .await;
    }

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?q=alpha",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let keys: Vec<String> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["alpha.txt".to_string()]);
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert_eq!(prefixes, vec!["folder/".to_string()]);

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?q=folder",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["files"].as_array().unwrap().is_empty());
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert_eq!(prefixes, vec!["folder/".to_string()]);

    s3_request(
        "PUT",
        &format!("{}/search-bucket/empty-archive/", base_url),
        vec![],
    )
    .await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?q=archive",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert!(prefixes.contains(&"empty-archive/".to_string()));

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?prefix=folder/&q=alpha",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let keys: Vec<String> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["folder/gamma-alpha.txt".to_string()]);
}

#[tokio::test]
async fn test_console_list_objects_returns_all_sibling_folders_on_first_page() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/sibling-bucket", base_url), vec![]).await;

    s3_request(
        "PUT",
        &format!("{}/sibling-bucket/a-file.txt", base_url),
        b"x".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/sibling-bucket/z-file.txt", base_url),
        b"x".to_vec(),
    )
    .await;

    for i in 0..250 {
        s3_request(
            "PUT",
            &format!("{}/sibling-bucket/big-folder/item-{:03}.txt", base_url, i),
            b"x".to_vec(),
        )
        .await;
    }

    s3_request(
        "PUT",
        &format!("{}/sibling-bucket/other-folder/", base_url),
        vec![],
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/sibling-bucket/third-folder/", base_url),
        vec![],
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!("{}/api/buckets/sibling-bucket/objects", base_url))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        prefixes,
        vec![
            "big-folder/".to_string(),
            "other-folder/".to_string(),
            "third-folder/".to_string(),
        ]
    );
    let keys: Vec<String> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        keys,
        vec!["a-file.txt".to_string(), "z-file.txt".to_string()]
    );
    assert!(body["nextContinuationToken"].is_null());
}

#[tokio::test]
async fn test_console_list_objects_omits_current_folder_marker() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/marker-bucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/marker-bucket/nested/", base_url),
        vec![],
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/marker-bucket/nested/file.txt", base_url),
        b"x".to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/marker-bucket/objects?prefix=nested/",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert!(!prefixes.contains(&"nested/".to_string()));
    let keys: Vec<String> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["nested/file.txt".to_string()]);
}

#[tokio::test]
async fn test_console_delete_object_missing_bucket_returns_404() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let resp = client()
        .delete(&format!(
            "{}/api/buckets/missing-bucket/objects/file.txt",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "Bucket not found");
}

#[tokio::test]
async fn test_console_presign_simple_key() {
    let base_url = start_server().await;

    // Create bucket and upload object via S3 API
    s3_request("PUT", &format!("{}/cpresign-bucket", base_url), vec![]).await;
    let body = b"console presign test";
    s3_request(
        "PUT",
        &format!("{}/cpresign-bucket/test.txt", base_url),
        body.to_vec(),
    )
    .await;

    // Login to console API
    let session = console_login(&base_url).await;

    // Generate presigned URL via console endpoint
    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-bucket/presign/test.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");

    // Fetch the presigned URL without any auth — should succeed
    let resp = client().get(presigned_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_console_presign_key_with_spaces() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/cpresign-space", base_url), vec![]).await;
    let body = b"file with spaces";
    // Upload with a key containing spaces (URL-encoded in the request)
    s3_request(
        "PUT",
        &format!("{}/cpresign-space/my%20file.txt", base_url),
        body.to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    // Request presigned URL for the key with spaces (URL-encoded in the API path)
    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-space/presign/my%20file.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");

    let resp = client().get(presigned_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL for key with spaces should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_console_presign_nested_key() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/cpresign-nested", base_url), vec![]).await;
    let body = b"nested key content";
    s3_request(
        "PUT",
        &format!("{}/cpresign-nested/folder/sub/file.txt", base_url),
        body.to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-nested/presign/folder/sub/file.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");

    let resp = client().get(presigned_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL for nested key should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_console_presign_uses_forwarded_host() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/cpresign-proxy", base_url), vec![]).await;
    let body = b"proxy presign test";
    s3_request(
        "PUT",
        &format!("{}/cpresign-proxy/test.txt", base_url),
        body.to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-proxy/presign/test.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .header("X-Forwarded-Host", "cdn.example.com")
        .header("X-Forwarded-Proto", "https")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");
    assert!(
        presigned_url.starts_with("https://cdn.example.com/cpresign-proxy/test.txt?"),
        "unexpected presigned URL: {presigned_url}"
    );

    let parsed = reqwest::Url::parse(presigned_url).unwrap();
    let path = parsed.path();
    let query = parsed.query().expect("presigned URL should have query");
    let resp = client()
        .get(format!("{base_url}{path}?{query}"))
        .header("Host", "cdn.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL behind proxy host should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}
