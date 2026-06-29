use crate::common::*;

#[tokio::test]
async fn test_delete_objects_batch() {
    // mc uses POST /{bucket}?delete to delete objects (DeleteObjects API)
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/a.txt", base_url),
        b"aaa".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/b.txt", base_url),
        b"bbb".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/c.txt", base_url),
        b"ccc".to_vec(),
    )
    .await;

    // Batch delete a.txt and b.txt
    let delete_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Delete>
  <Object><Key>a.txt</Key></Object>
  <Object><Key>b.txt</Key></Object>
</Delete>"#;

    let resp = s3_request(
        "POST",
        &format!("{}/mybucket?delete", base_url),
        delete_xml.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<Deleted>"),
        "Response should contain Deleted elements"
    );
    assert!(body.contains("<Key>a.txt</Key>"));
    assert!(body.contains("<Key>b.txt</Key>"));

    // Verify a.txt and b.txt are gone
    let resp = s3_request("GET", &format!("{}/mybucket/a.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
    let resp = s3_request("GET", &format!("{}/mybucket/b.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);

    // c.txt should still exist
    let resp = s3_request("GET", &format!("{}/mybucket/c.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_delete_objects_requires_delete_query_param() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/keep.txt", base_url),
        b"keep".to_vec(),
    )
    .await;

    let delete_xml = r#"<Delete><Object><Key>keep.txt</Key></Object></Delete>"#;
    let resp = s3_request(
        "POST",
        &format!("{}/mybucket", base_url),
        delete_xml.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 400);

    let get = s3_request("GET", &format!("{}/mybucket/keep.txt", base_url), vec![]).await;
    assert_eq!(get.status(), 200);
}

#[tokio::test]
async fn test_delete_objects_batch_missing_bucket_returns_404() {
    let base_url = start_server().await;

    let delete_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Delete>
  <Object><Key>a.txt</Key></Object>
</Delete>"#;

    let resp = s3_request(
        "POST",
        &format!("{}/missing-bucket?delete", base_url),
        delete_xml.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("NoSuchBucket"), "body: {}", body);
}

#[tokio::test]
async fn test_delete_objects_batch_with_version_id() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/ver-bucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/ver-bucket?versioning", base_url),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Status>Enabled</Status>
</VersioningConfiguration>"#
            .to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/ver-bucket/obj.txt", base_url),
        b"v1".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/ver-bucket/obj.txt", base_url),
        b"v2".to_vec(),
    )
    .await;

    let list_resp = s3_request("GET", &format!("{}/ver-bucket?versions", base_url), vec![]).await;
    let list_body = list_resp.text().await.unwrap();
    let old_vid = list_body
        .split("<VersionId>")
        .nth(2)
        .and_then(|s| s.split("</VersionId>").next())
        .expect("second version id");
    assert_ne!(old_vid, "null");

    let delete_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Delete>
  <Object><Key>obj.txt</Key><VersionId>{old_vid}</VersionId></Object>
</Delete>"#
    );
    let resp = s3_request(
        "POST",
        &format!("{}/ver-bucket?delete", base_url),
        delete_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Deleted>"), "body: {}", body);
    assert!(
        !body.contains("<Error>"),
        "batch delete should not report errors: {}",
        body
    );

    let list_resp = s3_request("GET", &format!("{}/ver-bucket?versions", base_url), vec![]).await;
    let list_body = list_resp.text().await.unwrap();
    assert!(
        !list_body.contains(&format!("<VersionId>{old_vid}</VersionId>")),
        "old version should be gone: {}",
        list_body
    );
}

#[tokio::test]
async fn test_list_object_versions_pagination() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/page-bucket", base_url), vec![]).await;
    for key in ["a.txt", "b.txt", "c.txt"] {
        s3_request(
            "PUT",
            &format!("{}/page-bucket/{key}", base_url),
            b"x".to_vec(),
        )
        .await;
    }

    let resp = s3_request(
        "GET",
        &format!("{}/page-bucket?versions&max-keys=2", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<IsTruncated>true</IsTruncated>"), "{}", body);
    assert!(body.contains("<NextKeyMarker>"), "{}", body);

    let marker = body
        .split("<NextKeyMarker>")
        .nth(1)
        .and_then(|s| s.split("</NextKeyMarker>").next())
        .expect("next key marker");
    let resp = s3_request(
        "GET",
        &format!(
            "{}/page-bucket?versions&max-keys=2&key-marker={marker}",
            base_url
        ),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<IsTruncated>false</IsTruncated>"),
        "{}",
        body
    );
}

#[tokio::test]
async fn test_trailing_slash_bucket_routes() {
    // mc sends PUT /bucket/ (with trailing slash)
    let base_url = start_server().await;

    // Create with trailing slash
    let resp = s3_request("PUT", &format!("{}/mybucket/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // HEAD with trailing slash
    let resp = s3_request("HEAD", &format!("{}/mybucket/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // GET (list) with trailing slash
    let resp = s3_request(
        "GET",
        &format!("{}/mybucket/?list-type=2", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);

    // DELETE with trailing slash
    let resp = s3_request("DELETE", &format!("{}/mybucket/", base_url), vec![]).await;
    assert_eq!(resp.status(), 204);
}
