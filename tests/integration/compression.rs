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

async fn console_request(
    base_url: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> (reqwest::StatusCode, serde_json::Value) {
    let session = console_login(base_url).await;
    let mut request = client()
        .request(method, format!("{base_url}{path}"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", base_url);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap_or_default())
}

async fn start_compress_job(
    base_url: &str,
    bucket: &str,
) -> (reqwest::StatusCode, serde_json::Value) {
    console_request(
        base_url,
        reqwest::Method::POST,
        "/api/jobs",
        Some(serde_json::json!({"kind": "compressBucket", "scope": bucket})),
    )
    .await
}

async fn wait_for_job(base_url: &str, id: &str) -> serde_json::Value {
    for _ in 0..200 {
        let (_, body) = console_request(
            base_url,
            reqwest::Method::GET,
            &format!("/api/jobs/{id}"),
            None,
        )
        .await;
        if body["job"]["status"] != "running" {
            return body["job"].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("job did not finish");
}

async fn run_rewrite_job(base_url: &str, bucket: &str) -> serde_json::Value {
    let (status, body) = start_compress_job(base_url, bucket).await;
    assert_eq!(status, 200);
    wait_for_job(base_url, body["job"]["id"].as_str().unwrap()).await
}

#[tokio::test]
async fn test_compress_existing_objects_job() {
    let base_url = start_server().await;
    let bucket = "lz4-rewrite";
    s3_request("PUT", &format!("{base_url}/{bucket}"), vec![]).await;

    let compressible_data = compressible(1_500_000);
    let small = compressible(500);
    let random: Vec<u8> = (0..300_000)
        .scan(0x9e3779b9u32, |state, _| {
            *state ^= *state << 13;
            *state ^= *state >> 17;
            *state ^= *state << 5;
            Some(*state as u8)
        })
        .collect();
    for (key, data) in [
        ("a/big.bin", &compressible_data),
        ("b/small.bin", &small),
        ("c/random.bin", &random),
    ] {
        s3_request("PUT", &format!("{base_url}/{bucket}/{key}"), data.clone()).await;
    }

    let (status, estimate) = console_request(
        &base_url,
        reqwest::Method::GET,
        &format!("/api/buckets/{bucket}/compression/estimate"),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(estimate["candidateObjects"], 2);
    assert!(estimate["ratio"].as_f64().unwrap() > 1.0);
    assert!(estimate["estimatedSavedBytes"].as_i64().unwrap() > 400_000);

    let job = run_rewrite_job(&base_url, bucket).await;
    assert_eq!(job["status"], "completed", "{job}");
    assert_eq!(job["detail"]["compressed"], 1, "{job}");
    assert_eq!(job["detail"]["failed"], 0, "{job}");
    assert!(
        job["detail"]["bytesAfter"].as_i64().unwrap()
            < job["detail"]["bytesBefore"].as_i64().unwrap() / 2
    );

    assert!(stored_len(base_url.data_dir(), bucket, "a/big.bin") < 750_000);
    assert_eq!(stored_len(base_url.data_dir(), bucket, "b/small.bin"), 500);
    assert_eq!(
        stored_len(base_url.data_dir(), bucket, "c/random.bin"),
        300_000
    );

    for (key, data) in [
        ("a/big.bin", &compressible_data),
        ("b/small.bin", &small),
        ("c/random.bin", &random),
    ] {
        let get = s3_request("GET", &format!("{base_url}/{bucket}/{key}"), vec![]).await;
        assert_eq!(get.bytes().await.unwrap().as_ref(), data.as_slice());
    }
    let range = s3_request_with_headers(
        "GET",
        &format!("{base_url}/{bucket}/a/big.bin"),
        vec![],
        vec![("range", "bytes=300000-900000")],
    )
    .await;
    assert_eq!(range.status(), 206);
    assert_eq!(
        range.bytes().await.unwrap().as_ref(),
        &compressible_data[300_000..=900_000]
    );

    // A second run only revisits the incompressible object and leaves it alone.
    let job = run_rewrite_job(&base_url, bucket).await;
    assert_eq!(job["detail"]["compressed"], 0, "{job}");
    assert_eq!(job["detail"]["skipped"], 1, "{job}");
}

#[tokio::test]
async fn test_compress_existing_versions() {
    let base_url = start_server().await;
    let bucket = "lz4-rewrite-versions";
    s3_request("PUT", &format!("{base_url}/{bucket}"), vec![]).await;
    s3_request(
        "PUT",
        &format!("{base_url}/{bucket}?versioning"),
        b"<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>".to_vec(),
    )
    .await;
    let v1 = compressible(200_000);
    let v2: Vec<u8> = compressible(250_000).iter().map(|b| b ^ 0x33).collect();
    let put1 = s3_request("PUT", &format!("{base_url}/{bucket}/k"), v1.clone()).await;
    let vid1 = put1.headers()["x-amz-version-id"]
        .to_str()
        .unwrap()
        .to_string();
    s3_request("PUT", &format!("{base_url}/{bucket}/k"), v2.clone()).await;

    let job = run_rewrite_job(&base_url, bucket).await;
    assert_eq!(job["status"], "completed", "{job}");
    // Two versions plus the current object.
    assert_eq!(job["detail"]["compressed"], 3, "{job}");

    let old = s3_request(
        "GET",
        &format!("{base_url}/{bucket}/k?versionId={vid1}"),
        vec![],
    )
    .await;
    assert_eq!(old.bytes().await.unwrap().as_ref(), v1.as_slice());
    let cur = s3_request("GET", &format!("{base_url}/{bucket}/k"), vec![]).await;
    assert_eq!(cur.bytes().await.unwrap().as_ref(), v2.as_slice());
}

#[tokio::test]
async fn test_rewrite_job_rejects_second_start_and_survives_overwrites() {
    let base_url = start_server().await;
    let bucket = "lz4-rewrite-race";
    s3_request("PUT", &format!("{base_url}/{bucket}"), vec![]).await;
    let data = compressible(300_000);
    for i in 0..60 {
        s3_request("PUT", &format!("{base_url}/{bucket}/k{i:03}"), data.clone()).await;
    }

    let (status, started) = start_compress_job(&base_url, bucket).await;
    assert_eq!(status, 200);
    let job_id = started["job"]["id"].as_str().unwrap().to_string();

    // Overwrite and delete objects while the job runs.
    let fresh: Vec<u8> = compressible(310_000).iter().map(|b| b ^ 0x7f).collect();
    for i in 0..60 {
        let url = format!("{base_url}/{bucket}/k{i:03}");
        if i % 3 == 0 {
            s3_request("PUT", &url, fresh.clone()).await;
        } else if i % 3 == 1 {
            s3_request("DELETE", &url, vec![]).await;
        }
    }
    let (second, _) = start_compress_job(&base_url, bucket).await;
    assert!(second == 409 || second == 200);

    let job = wait_for_job(&base_url, &job_id).await;
    assert_eq!(job["status"], "completed", "{job}");
    assert_eq!(job["detail"]["failed"], 0, "{job}");

    for i in 0..60 {
        let get = s3_request("GET", &format!("{base_url}/{bucket}/k{i:03}"), vec![]).await;
        match i % 3 {
            0 => assert_eq!(get.bytes().await.unwrap().as_ref(), fresh.as_slice()),
            1 => assert_eq!(get.status(), 404),
            _ => assert_eq!(get.bytes().await.unwrap().as_ref(), data.as_slice()),
        }
    }
}
