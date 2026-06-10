use crate::common::*;

#[tokio::test]
async fn test_prometheus_metrics_endpoint() {
    let base_url = start_server_with_metrics_token().await;

    assert_eq!(
        s3_request("PUT", &format!("{}/prom-bucket", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/prom-bucket/obj.txt", base_url),
            b"metrics probe".to_vec(),
        )
        .await
        .status(),
        200
    );

    let resp = client()
        .get(format!("{}/metrics", base_url))
        .header("authorization", "Bearer metrics-test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("maxio_http_requests_total"));
}
