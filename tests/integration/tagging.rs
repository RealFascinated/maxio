use crate::common::*;

#[tokio::test]
async fn test_put_and_get_object_tagging() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/tag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/tag-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;

    let tagging_xml = r#"<Tagging><TagSet><Tag><Key>env</Key><Value>prod</Value></Tag><Tag><Key>team</Key><Value>platform</Value></Tag></TagSet></Tagging>"#;
    let resp = s3_request(
        "PUT",
        &format!("{}/tag-bucket/obj.txt?tagging", base),
        tagging_xml.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let resp = s3_request(
        "GET",
        &format!("{}/tag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Key>env</Key>"));
    assert!(body.contains("<Value>prod</Value>"));
    assert!(body.contains("<Key>team</Key>"));
    assert!(body.contains("<Value>platform</Value>"));
}

#[tokio::test]
async fn test_get_object_tagging_no_tags() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/notag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/notag-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;

    let resp = s3_request(
        "GET",
        &format!("{}/notag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Tagging>") || body.contains("<TagSet"));
    assert!(!body.contains("<Tag>"));
}

#[tokio::test]
async fn test_delete_object_tagging() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/deltag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/deltag-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;

    let tagging_xml =
        r#"<Tagging><TagSet><Tag><Key>env</Key><Value>prod</Value></Tag></TagSet></Tagging>"#;
    s3_request(
        "PUT",
        &format!("{}/deltag-bucket/obj.txt?tagging", base),
        tagging_xml.as_bytes().to_vec(),
    )
    .await;

    let resp = s3_request(
        "DELETE",
        &format!("{}/deltag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 204);

    let resp = s3_request(
        "GET",
        &format!("{}/deltag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(!body.contains("<Tag>"));
}

#[tokio::test]
async fn test_get_object_tagging_no_such_key() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/nsk-bucket", base), vec![]).await;

    let resp = s3_request(
        "GET",
        &format!("{}/nsk-bucket/nonexistent.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("NoSuchKey"));
}

#[tokio::test]
async fn test_put_object_tagging_too_many_tags() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/manytagbucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/manytagbucket/obj.txt", base),
        b"data".to_vec(),
    )
    .await;

    let tags: String = (1..=11)
        .map(|i| format!("<Tag><Key>key{}</Key><Value>val{}</Value></Tag>", i, i))
        .collect();
    let tagging_xml = format!("<Tagging><TagSet>{}</TagSet></Tagging>", tags);
    let resp = s3_request(
        "PUT",
        &format!("{}/manytagbucket/obj.txt?tagging", base),
        tagging_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

#[tokio::test]
async fn test_put_object_tagging_key_too_long() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/longtag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/longtag-bucket/obj.txt", base),
        b"data".to_vec(),
    )
    .await;

    let long_key = "k".repeat(129);
    let tagging_xml = format!(
        "<Tagging><TagSet><Tag><Key>{}</Key><Value>v</Value></Tag></TagSet></Tagging>",
        long_key
    );
    let resp = s3_request(
        "PUT",
        &format!("{}/longtag-bucket/obj.txt?tagging", base),
        tagging_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

#[tokio::test]
async fn test_put_object_tagging_value_too_long() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/longval-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/longval-bucket/obj.txt", base),
        b"data".to_vec(),
    )
    .await;

    let long_val = "v".repeat(257);
    let tagging_xml = format!(
        "<Tagging><TagSet><Tag><Key>k</Key><Value>{}</Value></Tag></TagSet></Tagging>",
        long_val
    );
    let resp = s3_request(
        "PUT",
        &format!("{}/longval-bucket/obj.txt?tagging", base),
        tagging_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

// UploadPartCopy: copy entire source object as a multipart part
