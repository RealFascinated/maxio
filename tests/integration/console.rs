use crate::common::*;

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
async fn test_console_list_objects_sort_by_size_desc() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/sort-bucket", base_url), vec![]).await;

    for (key, body) in [
        ("small.txt", b"1".as_slice()),
        ("large.txt", b"0123456789".as_slice()),
        ("medium.txt", b"12345".as_slice()),
    ] {
        s3_request(
            "PUT",
            &format!("{}/sort-bucket/{}", base_url, key),
            body.to_vec(),
        )
        .await;
    }

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/sort-bucket/objects?sort=size&order=desc",
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
    assert_eq!(
        keys,
        vec![
            "large.txt".to_string(),
            "medium.txt".to_string(),
            "small.txt".to_string(),
        ]
    );
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
async fn test_console_delete_objects_batch() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/batch-del-bucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/batch-del-bucket/a.txt", base_url),
        b"aaa".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/batch-del-bucket/b.txt", base_url),
        b"bbb".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/batch-del-bucket/keep.txt", base_url),
        b"keep".to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .post(format!(
            "{}/api/buckets/batch-del-bucket/objects/delete",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .json(&serde_json::json!({ "keys": ["a.txt", "b.txt"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["deleted"], 2);
    assert!(body["failed"].as_array().unwrap().is_empty());

    let resp = client()
        .get(format!(
            "{}/api/buckets/batch-del-bucket/objects?prefix=",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: serde_json::Value = resp.json().await.unwrap();
    let keys: Vec<String> = list["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["keep.txt".to_string()]);
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
async fn test_console_get_object_detail() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/detail-bucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/detail-bucket/nested/file.txt", base_url),
        b"detail test".to_vec(),
    )
    .await;
    let tagging_xml =
        r#"<Tagging><TagSet><Tag><Key>env</Key><Value>prod</Value></Tag></TagSet></Tagging>"#;
    s3_request(
        "PUT",
        &format!("{}/detail-bucket/nested/file.txt?tagging", base_url),
        tagging_xml.as_bytes().to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/detail-bucket/objects/nested/file.txt",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["key"], "nested/file.txt");
    assert_eq!(body["size"], 11);
    assert_eq!(body["contentType"], "application/octet-stream");
    assert!(body["etag"].is_string());
    assert_eq!(body["tags"]["env"], "prod");
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

#[tokio::test]
async fn test_console_presign_uses_iam_credentials() {
    let base_url = start_server().await;
    let root_session = console_login(&base_url).await;

    s3_request("PUT", &format!("{}/iam-presign-bucket", base_url), vec![]).await;
    let body = b"iam presign test";
    s3_request(
        "PUT",
        &format!("{}/iam-presign-bucket/test.txt", base_url),
        body.to_vec(),
    )
    .await;

    let create = client()
        .post(format!("{base_url}/api/users"))
        .header("cookie", format!("maxio_session={root_session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({"username": "presign-user"}))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);
    let create_body: serde_json::Value = create.json().await.unwrap();
    let access_key = create_body["accessKey"]["accessKeyId"]
        .as_str()
        .unwrap()
        .to_string();
    let secret_key = create_body["accessKey"]["secretAccessKey"]
        .as_str()
        .unwrap()
        .to_string();

    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:GetObject","s3:ListBucket"],"Resource":["arn:aws:s3:::iam-presign-bucket","arn:aws:s3:::iam-presign-bucket/*"]}]}"#;
    let put_policy = client()
        .put(format!(
            "{base_url}/api/users/presign-user/policies/read-access"
        ))
        .header("cookie", format!("maxio_session={root_session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "document": policy }))
        .send()
        .await
        .unwrap();
    assert_eq!(put_policy.status(), 200);

    let session = console_login_with_creds(&base_url, &access_key, &secret_key).await;

    let resp = client()
        .get(&format!(
            "{}/api/buckets/iam-presign-bucket/presign/test.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");

    let parsed = reqwest::Url::parse(presigned_url).unwrap();
    let credential = parsed
        .query_pairs()
        .find(|(k, _)| k == "X-Amz-Credential")
        .map(|(_, v)| v.into_owned())
        .expect("presigned URL should include X-Amz-Credential");
    assert!(
        credential.starts_with(&format!("{access_key}/")),
        "expected IAM access key {access_key} in credential, got {credential}"
    );
    assert!(
        !credential.starts_with(&format!("{ACCESS_KEY}/")),
        "presigned URL should not use root credentials"
    );

    let resp = client().get(presigned_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "IAM presigned URL should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_console_delete_folder() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/folder-del-bucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/folder-del-bucket/photos/", base_url),
        vec![],
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/folder-del-bucket/photos/vacation.jpg", base_url),
        b"photo".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/folder-del-bucket/photos/nested/note.txt", base_url),
        b"note".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/folder-del-bucket/other.txt", base_url),
        b"keep".to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .post(format!(
            "{}/api/buckets/folder-del-bucket/folders/preview",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .json(&serde_json::json!({ "names": ["photos/"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let preview: serde_json::Value = resp.json().await.unwrap();
    assert!(preview["count"].as_u64().unwrap() >= 3);
    assert!(preview["sizeBytes"].as_u64().unwrap() > 0);

    let resp = client()
        .delete(format!(
            "{}/api/buckets/folder-del-bucket/folders",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .json(&serde_json::json!({ "name": "photos/" }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body_text = resp.text().await.unwrap();
    assert_eq!(status, 200, "body: {body_text}");
    let body: serde_json::Value = serde_json::from_str(&body_text).unwrap();
    assert!(body["deleted"].as_u64().unwrap() >= 3);

    let resp = client()
        .delete(format!(
            "{}/api/buckets/folder-del-bucket/folders",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .json(&serde_json::json!({ "name": "photos/" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let empty_body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(empty_body["deleted"], 0);

    let resp = client()
        .get(format!(
            "{}/api/buckets/folder-del-bucket/objects?prefix=",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: serde_json::Value = resp.json().await.unwrap();
    let keys: Vec<String> = list["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["other.txt".to_string()]);
    let prefixes: Vec<String> = list["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert!(prefixes.is_empty());
}
