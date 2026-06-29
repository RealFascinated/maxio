use crate::common::*;

#[tokio::test]
async fn test_upload_part_copy_full() {
    let base = start_server().await;

    // Create source bucket and object
    s3_request("PUT", &format!("{}/src-upc", base), vec![]).await;
    let src_data: Vec<u8> = (0u8..255).cycle().take(5 * 1024 * 1024).collect(); // 5 MiB
    s3_request(
        "PUT",
        &format!("{}/src-upc/source.bin", base),
        src_data.clone(),
    )
    .await;

    // Create destination bucket and start multipart upload
    s3_request("PUT", &format!("{}/dst-upc", base), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/dst-upc/dest.bin?uploads=", base),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    // UploadPartCopy: copy full source as part 1
    let resp = s3_request_with_headers(
        "PUT",
        &format!(
            "{}/dst-upc/dest.bin?partNumber=1&uploadId={}",
            base, upload_id
        ),
        vec![],
        vec![("x-amz-copy-source", "/src-upc/source.bin")],
    )
    .await;
    assert_eq!(resp.status(), 200, "upload_part_copy should return 200");
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<CopyPartResult"),
        "response should be CopyPartResult XML, got: {}",
        body
    );
    let etag = extract_xml_tag(&body, "ETag").unwrap();
    assert!(
        etag.starts_with('"') && etag.ends_with('"'),
        "ETag should be quoted"
    );

    // Complete the multipart upload
    let complete_xml = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        etag
    );
    let complete = s3_request(
        "POST",
        &format!("{}/dst-upc/dest.bin?uploadId={}", base, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    assert_eq!(complete.status(), 200);

    // Verify content matches source
    let get = s3_request("GET", &format!("{}/dst-upc/dest.bin", base), vec![]).await;
    assert_eq!(get.status(), 200);
    assert_eq!(get.bytes().await.unwrap().as_ref(), src_data.as_slice());
}

// UploadPartCopy: copy a byte range from source object as a multipart part
#[tokio::test]
async fn test_upload_part_copy_range() {
    let base = start_server().await;

    // Create source with known content
    s3_request("PUT", &format!("{}/src-upcr", base), vec![]).await;
    // part1: 5 MiB of 'A', part2: 1 KiB of 'B'
    let part1: Vec<u8> = vec![b'A'; 5 * 1024 * 1024];
    let part2: Vec<u8> = vec![b'B'; 1024];
    let mut src_data = part1.clone();
    src_data.extend_from_slice(&part2);
    s3_request(
        "PUT",
        &format!("{}/src-upcr/source.bin", base),
        src_data.clone(),
    )
    .await;

    // Create destination and start multipart upload
    s3_request("PUT", &format!("{}/dst-upcr", base), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/dst-upcr/dest.bin?uploads=", base),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    // Part 1: bytes 0 to (5MiB - 1)
    let r1 = s3_request_with_headers(
        "PUT",
        &format!(
            "{}/dst-upcr/dest.bin?partNumber=1&uploadId={}",
            base, upload_id
        ),
        vec![],
        vec![
            ("x-amz-copy-source", "/src-upcr/source.bin"),
            (
                "x-amz-copy-source-range",
                &format!("bytes=0-{}", 5 * 1024 * 1024 - 1),
            ),
        ],
    )
    .await;
    assert_eq!(r1.status(), 200);
    let body1 = r1.text().await.unwrap();
    assert!(body1.contains("<CopyPartResult"));
    let e1 = extract_xml_tag(&body1, "ETag").unwrap();

    // Part 2: remaining bytes
    let r2 = s3_request_with_headers(
        "PUT",
        &format!(
            "{}/dst-upcr/dest.bin?partNumber=2&uploadId={}",
            base, upload_id
        ),
        vec![],
        vec![
            ("x-amz-copy-source", "/src-upcr/source.bin"),
            (
                "x-amz-copy-source-range",
                &format!("bytes={}-{}", 5 * 1024 * 1024, src_data.len() - 1),
            ),
        ],
    )
    .await;
    assert_eq!(r2.status(), 200);
    let body2 = r2.text().await.unwrap();
    assert!(body2.contains("<CopyPartResult"));
    let e2 = extract_xml_tag(&body2, "ETag").unwrap();

    // Complete
    let complete_xml = format!(
        "<CompleteMultipartUpload>\
            <Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part>\
            <Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part>\
        </CompleteMultipartUpload>",
        e1, e2
    );
    let complete = s3_request(
        "POST",
        &format!("{}/dst-upcr/dest.bin?uploadId={}", base, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    assert_eq!(complete.status(), 200);

    // Verify reconstructed content matches original source
    let get = s3_request("GET", &format!("{}/dst-upcr/dest.bin", base), vec![]).await;
    assert_eq!(get.status(), 200);
    assert_eq!(get.bytes().await.unwrap().as_ref(), src_data.as_slice());
}
