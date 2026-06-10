use crate::common::*;

#[tokio::test]
async fn test_get_object_conditional_headers() {
    let base_url = start_server().await;
    let bucket = "cond-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/cond.txt", base_url),
            b"conditional test".to_vec(),
        )
        .await
        .status(),
        200
    );

    let head = s3_request("HEAD", &format!("{}/{bucket}/cond.txt", base_url), vec![]).await;
    assert_eq!(head.status(), 200);
    let etag = head
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let ok = s3_request_with_headers(
        "GET",
        &format!("{}/{bucket}/cond.txt", base_url),
        vec![],
        vec![("if-match", &etag)],
    )
    .await;
    assert_eq!(ok.status(), 200);

    let bad_match = s3_request_with_headers(
        "GET",
        &format!("{}/{bucket}/cond.txt", base_url),
        vec![],
        vec![("if-match", "\"wrongetag000000000000000000000000\"")],
    )
    .await;
    assert_eq!(bad_match.status(), 412);

    let not_modified = s3_request_with_headers(
        "GET",
        &format!("{}/{bucket}/cond.txt", base_url),
        vec![],
        vec![("if-none-match", &etag)],
    )
    .await;
    assert_eq!(not_modified.status(), 304);

    let ok_none = s3_request_with_headers(
        "GET",
        &format!("{}/{bucket}/cond.txt", base_url),
        vec![],
        vec![("if-none-match", "\"wrongetag000000000000000000000000\"")],
    )
    .await;
    assert_eq!(ok_none.status(), 200);
}

#[tokio::test]
async fn test_head_object_conditional_if_match() {
    let base_url = start_server().await;
    let bucket = "cond-head-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/file.txt", base_url),
            b"data".to_vec(),
        )
        .await
        .status(),
        200
    );

    let head = s3_request("HEAD", &format!("{}/{bucket}/file.txt", base_url), vec![]).await;
    let etag = head
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let ok = s3_request_with_headers(
        "HEAD",
        &format!("{}/{bucket}/file.txt", base_url),
        vec![],
        vec![("if-match", &etag)],
    )
    .await;
    assert_eq!(ok.status(), 200);

    let fail = s3_request_with_headers(
        "HEAD",
        &format!("{}/{bucket}/file.txt", base_url),
        vec![],
        vec![("if-match", "\"bad\"")],
    )
    .await;
    assert_eq!(fail.status(), 412);
}
