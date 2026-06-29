use crate::common::*;

#[tokio::test]
async fn test_put_get_delete_bucket_lifecycle() {
    let base_url = start_server().await;
    let bucket = "lc-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let config = br#"<LifecycleConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Rule>
    <ID>expire-logs</ID>
    <Status>Enabled</Status>
    <Filter><Prefix>logs/</Prefix></Filter>
    <Expiration><Days>30</Days></Expiration>
  </Rule>
</LifecycleConfiguration>"#;

    let put = s3_request(
        "PUT",
        &format!("{}/{bucket}?lifecycle", base_url),
        config.to_vec(),
    )
    .await;
    assert_eq!(put.status(), 200);

    let get = s3_request("GET", &format!("{}/{bucket}?lifecycle", base_url), vec![]).await;
    assert_eq!(get.status(), 200);
    let body = get.text().await.unwrap();
    assert!(body.contains("<ID>expire-logs</ID>"));
    assert!(body.contains("<Days>30</Days>"));
    assert!(body.contains("<Prefix>logs/</Prefix>"));

    let del = s3_request(
        "DELETE",
        &format!("{}/{bucket}?lifecycle", base_url),
        vec![],
    )
    .await;
    assert_eq!(del.status(), 204);

    let get_missing = s3_request("GET", &format!("{}/{bucket}?lifecycle", base_url), vec![]).await;
    assert_eq!(get_missing.status(), 404);
}

#[tokio::test]
async fn test_console_lifecycle_json_round_trip() {
    let base_url = start_server().await;
    let bucket = "lc-console-bucket";
    let session = console_login(&base_url).await;

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let put = reqwest::Client::new()
        .put(format!("{}/api/buckets/{bucket}/lifecycle", base_url))
        .header("cookie", format!("maxio_session={}", session))
        .json(&serde_json::json!({
            "rules": [{
                "id": "expire-all",
                "enabled": true,
                "actions": [{ "type": "expire_objects", "days": 14 }]
            }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 200);

    let get = reqwest::Client::new()
        .get(format!("{}/api/buckets/{bucket}/lifecycle", base_url))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let body: serde_json::Value = get.json().await.unwrap();
    assert_eq!(body["rules"][0]["id"], "expire-all");
    assert_eq!(body["rules"][0]["actions"][0]["days"], 14);
}
