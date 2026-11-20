use crate::common::*;

fn compressible(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i / 13) % 97) as u8).collect()
}

async fn enable_compression(base_url: &str, bucket: &str) {
    let session = console_login(base_url).await;
    let set = client()
        .put(format!("{base_url}/api/buckets/{bucket}/compression"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", base_url)
        .json(&serde_json::json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(set.status(), 200);
}

fn stored_len(data_dir: &str, bucket: &str, key: &str) -> u64 {
    std::fs::metadata(format!("{data_dir}/buckets/{bucket}/{key}"))
        .unwrap()
        .len()
}

#[tokio::test]
async fn test_compressed_put_get_and_range() {
    let base_url = start_server().await;
    let bucket = "lz4-bucket";
    s3_request("PUT", &format!("{base_url}/{bucket}"), vec![]).await;
    enable_compression(&base_url, bucket).await;

    for (key, len) in [("small.bin", 4000), ("large.bin", 3 * 1024 * 1024 + 123)] {
        let data = compressible(len);
        let put = s3_request("PUT", &format!("{base_url}/{bucket}/{key}"), data.clone()).await;
        assert_eq!(put.status(), 200);
        assert!(stored_len(base_url.data_dir(), bucket, key) < len as u64 / 2);

        let get = s3_request("GET", &format!("{base_url}/{bucket}/{key}"), vec![]).await;
        assert_eq!(get.headers()["content-length"], len.to_string());
        assert_eq!(get.bytes().await.unwrap().as_ref(), data.as_slice());

        let start = len / 3;
        let end = len / 3 + 700_000.min(len / 2);
        let range = s3_request_with_headers(
            "GET",
            &format!("{base_url}/{bucket}/{key}"),
            vec![],
            vec![("range", &format!("bytes={start}-{end}"))],
        )
        .await;
        assert_eq!(range.status(), 206);
        assert_eq!(range.bytes().await.unwrap().as_ref(), &data[start..=end]);
    }
}

#[tokio::test]
async fn test_compression_only_applies_to_new_objects() {
    let base_url = start_server().await;
    let bucket = "lz4-new-only";
    s3_request("PUT", &format!("{base_url}/{bucket}"), vec![]).await;
    let data = compressible(100_000);
    s3_request("PUT", &format!("{base_url}/{bucket}/before"), data.clone()).await;
    enable_compression(&base_url, bucket).await;
    s3_request("PUT", &format!("{base_url}/{bucket}/after"), data.clone()).await;

    assert_eq!(stored_len(base_url.data_dir(), bucket, "before"), 100_000);
    assert!(stored_len(base_url.data_dir(), bucket, "after") < 50_000);
    for key in ["before", "after"] {
        let get = s3_request("GET", &format!("{base_url}/{bucket}/{key}"), vec![]).await;
        assert_eq!(get.bytes().await.unwrap().as_ref(), data.as_slice());
    }
}

#[tokio::test]
async fn test_compressed_multipart_copy_and_versions() {
    let base_url = start_server().await;
    let bucket = "lz4-multipart";
    s3_request("PUT", &format!("{base_url}/{bucket}"), vec![]).await;
    s3_request("PUT", &format!("{base_url}/plain-copy"), vec![]).await;
    enable_compression(&base_url, bucket).await;

    let create = s3_request(
        "POST",
        &format!("{base_url}/{bucket}/mp.bin?uploads="),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();
    let p1 = compressible(5 * 1024 * 1024);
    let p2 = compressible(777_777);
    let mut etags = Vec::new();
    for (n, part) in [(1, &p1), (2, &p2)] {
        let r = s3_request(
            "PUT",
            &format!("{base_url}/{bucket}/mp.bin?partNumber={n}&uploadId={upload_id}"),
            part.clone(),
        )
        .await;
        etags.push(r.headers()["etag"].to_str().unwrap().to_string());
    }
    let complete = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        etags[0], etags[1]
    );
    let done = s3_request(
        "POST",
        &format!("{base_url}/{bucket}/mp.bin?uploadId={upload_id}"),
        complete.into_bytes(),
    )
    .await;
    assert_eq!(done.status(), 200);

    let mut expected = p1;
    expected.extend_from_slice(&p2);
    assert!(stored_len(base_url.data_dir(), bucket, "mp.bin") < expected.len() as u64 / 2);
    let get = s3_request("GET", &format!("{base_url}/{bucket}/mp.bin"), vec![]).await;
    assert_eq!(get.bytes().await.unwrap().as_ref(), expected.as_slice());

    // Copy into an uncompressed bucket stores the object raw.
    let copy = s3_request_with_headers(
        "PUT",
        &format!("{base_url}/plain-copy/mp.bin"),
        vec![],
        vec![("x-amz-copy-source", &format!("/{bucket}/mp.bin"))],
    )
    .await;
    assert_eq!(copy.status(), 200);
    assert_eq!(
        stored_len(base_url.data_dir(), "plain-copy", "mp.bin"),
        expected.len() as u64
    );
    let get = s3_request("GET", &format!("{base_url}/plain-copy/mp.bin"), vec![]).await;
    assert_eq!(get.bytes().await.unwrap().as_ref(), expected.as_slice());
}

#[tokio::test]
async fn test_compressed_versions_round_trip() {
    let base_url = start_server().await;
    let bucket = "lz4-versions";
    s3_request("PUT", &format!("{base_url}/{bucket}"), vec![]).await;
    enable_compression(&base_url, bucket).await;
    s3_request(
        "PUT",
        &format!("{base_url}/{bucket}?versioning"),
        b"<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>".to_vec(),
    )
    .await;

    let v1 = compressible(400_000);
    let v2: Vec<u8> = compressible(500_000).iter().map(|b| b ^ 0x5a).collect();
    let put1 = s3_request("PUT", &format!("{base_url}/{bucket}/k"), v1.clone()).await;
    let vid1 = put1.headers()["x-amz-version-id"]
        .to_str()
        .unwrap()
        .to_string();
    s3_request("PUT", &format!("{base_url}/{bucket}/k"), v2.clone()).await;

    let old = s3_request(
        "GET",
        &format!("{base_url}/{bucket}/k?versionId={vid1}"),
        vec![],
    )
    .await;
    assert_eq!(old.bytes().await.unwrap().as_ref(), v1.as_slice());
    let cur = s3_request("GET", &format!("{base_url}/{bucket}/k"), vec![]).await;
    assert_eq!(cur.bytes().await.unwrap().as_ref(), v2.as_slice());

    // Deleting the newest version restores the previous (compressed) one as current.
    let list = s3_request("GET", &format!("{base_url}/{bucket}?versions"), vec![]).await;
    let xml = list.text().await.unwrap();
    let latest = extract_xml_tag(&xml, "VersionId").unwrap();
    s3_request(
        "DELETE",
        &format!("{base_url}/{bucket}/k?versionId={latest}"),
        vec![],
    )
    .await;
    let cur = s3_request("GET", &format!("{base_url}/{bucket}/k"), vec![]).await;
    assert_eq!(cur.bytes().await.unwrap().as_ref(), v1.as_slice());
}
