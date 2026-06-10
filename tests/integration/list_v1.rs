use crate::common::*;

#[tokio::test]
async fn test_list_objects_v1_marker_pagination() {
    let base_url = start_server().await;
    let bucket = "v1-pag-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    for i in 0..7 {
        assert_eq!(
            s3_request(
                "PUT",
                &format!("{}/{bucket}/obj{i}", base_url),
                format!("body{i}").into_bytes(),
            )
            .await
            .status(),
            200
        );
    }

    let page1 = s3_request("GET", &format!("{}/{bucket}?max-keys=3", base_url), vec![]).await;
    assert_eq!(page1.status(), 200);
    let body1 = page1.text().await.unwrap();
    assert!(body1.contains("<IsTruncated>true</IsTruncated>"));
    assert_eq!(body1.matches("<Key>").count(), 3);
    let marker = extract_xml_tag(&body1, "NextMarker").expect("next marker");

    let page2 = s3_request(
        "GET",
        &format!("{}/{bucket}?max-keys=3&marker={marker}", base_url),
        vec![],
    )
    .await;
    assert_eq!(page2.status(), 200);
    let body2 = page2.text().await.unwrap();
    assert_eq!(body2.matches("<Key>").count(), 3);

    let page3 = s3_request(
        "GET",
        &format!(
            "{}/{bucket}?max-keys=3&marker={}",
            base_url,
            extract_xml_tag(&body2, "NextMarker").unwrap()
        ),
        vec![],
    )
    .await;
    let body3 = page3.text().await.unwrap();
    assert_eq!(body3.matches("<Key>").count(), 1);
    assert!(body3.contains("<IsTruncated>false</IsTruncated>"));
}
