use std::sync::Arc;

use crate::common::*;
use maxio::server;
use tempfile::TempDir;

#[tokio::test]
async fn test_default_buckets_created_on_boot() {
    let base_url = start_server_with_default_buckets("alpha,beta,gamma").await;

    // All buckets should exist
    let resp = s3_request("HEAD", &format!("{}/alpha", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let resp = s3_request("HEAD", &format!("{}/beta", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let resp = s3_request("HEAD", &format!("{}/gamma", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // List should include default buckets
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Name>alpha</Name>"));
    assert!(body.contains("<Name>beta</Name>"));
    assert!(body.contains("<Name>gamma</Name>"));
}

#[tokio::test]
async fn test_default_buckets_skip_existing() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap().to_string();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(&data_dir, &database_url).await;

    // First provision: creates the bucket
    maxio::storage::provision_default_buckets(storage.as_ref(), "existing").await;
    // Second provision: must be idempotent — no error, no duplicate
    maxio::storage::provision_default_buckets(storage.as_ref(), "existing").await;

    let config = test_config(data_dir.clone(), database_url, "");
    let state = test_app_state(storage, Arc::new(config)).await;
    let _postgres = postgres;
    let app = server::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let resp = s3_request("HEAD", &format!("{}/existing", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Name>existing</Name>"));
}

#[tokio::test]
async fn test_default_buckets_skips_invalid_names() {
    let base_url =
        start_server_with_default_buckets("INVALID,valid,b..a,a.-b,a-.b,192.168.0.1").await;

    for bucket in ["INVALID", "b..a", "a.-b", "a-.b", "192.168.0.1"] {
        let resp = s3_request("HEAD", &format!("{}/{}", base_url, bucket), vec![]).await;
        assert_eq!(resp.status(), 404, "{bucket} should be skipped");
    }

    let resp = s3_request("HEAD", &format!("{}/valid", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_empty_default_buckets() {
    let base_url = start_server_with_default_buckets("").await;

    // No default buckets should exist
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(!body.contains("<Name>"), "No buckets should be listed");
}

#[tokio::test]
async fn test_default_buckets_single() {
    let base_url = start_server_with_default_buckets("only-one").await;

    let resp = s3_request("HEAD", &format!("{}/only-one", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}
