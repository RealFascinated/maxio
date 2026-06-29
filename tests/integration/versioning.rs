use crate::common::*;

#[tokio::test]
async fn test_put_get_bucket_versioning() {
    let base_url = start_server().await;
    let bucket = "ver-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let enable = s3_request(
        "PUT",
        &format!("{}/{bucket}?versioning", base_url),
        br#"<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Status>Enabled</Status></VersioningConfiguration>"#.to_vec(),
    )
    .await;
    assert_eq!(enable.status(), 200);

    let get = s3_request("GET", &format!("{}/{bucket}?versioning", base_url), vec![]).await;
    assert_eq!(get.status(), 200);
    let body = get.text().await.unwrap();
    assert!(body.contains("<Status>Enabled</Status>"));

    let put1 = s3_request("PUT", &format!("{}/{bucket}/obj", base_url), b"v1".to_vec()).await;
    assert_eq!(put1.status(), 200);
    let vid1 = put1
        .headers()
        .get("x-amz-version-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let put2 = s3_request("PUT", &format!("{}/{bucket}/obj", base_url), b"v2".to_vec()).await;
    let vid2 = put2
        .headers()
        .get("x-amz-version-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_ne!(vid1, vid2);

    let hist = s3_request(
        "GET",
        &format!("{}/{bucket}/obj?versionId={vid1}", base_url),
        vec![],
    )
    .await;
    assert_eq!(hist.status(), 200);
    assert_eq!(&hist.bytes().await.unwrap()[..], b"v1");

    let suspend = s3_request(
        "PUT",
        &format!("{}/{bucket}?versioning", base_url),
        br#"<VersioningConfiguration><Status>Suspended</Status></VersioningConfiguration>"#
            .to_vec(),
    )
    .await;
    assert_eq!(suspend.status(), 200);

    let get_suspended =
        s3_request("GET", &format!("{}/{bucket}?versioning", base_url), vec![]).await;
    assert_eq!(get_suspended.status(), 200);
    let suspended_body = get_suspended.text().await.unwrap();
    assert!(suspended_body.contains("<Status>Suspended</Status>"));
}
